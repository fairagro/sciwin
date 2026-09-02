#!/usr/bin/env node
// Regenerates the archived documentation snapshots consumed by the `starlight-versions`
// Starlight plugin (see docs/astro.config.mjs) from this repo's release tags.
//
// For every "X.Y" minor version, the latest "X.Y.Z" tag that has a docs/ site is treated as
// that version's canonical content. docs/versions.json records which tag each archived "X.Y"
// slug was last generated from; when a newer patch tag appears for an already-archived minor,
// its snapshot is regenerated from that newer tag. Versions are (re)built oldest-first because
// `starlight-versions` only supports archiving one new/missing version per build.
//
// Archived snapshots are written into docs/src/content/docs/<slug>/ (and the matching
// docs/src/content/versions/<slug>.json sidebar config) as real files meant to be committed --
// released docs never change, so there is no need to regenerate them on every build.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const docsRoot = path.resolve(import.meta.dirname, '..');
const repoRoot = path.resolve(docsRoot, '..');
const versionsFile = path.join(docsRoot, 'versions.json');

// Paths (repo-relative) whose content represents "the current live docs" and therefore needs
// to be overlaid with a historical tag's content while archiving that tag's version.
//
// `content/docs` is replaced exactly (files absent from the tag are removed) since it holds
// only versioned page content. `assets`/`public` are overlaid additively (existing files are
// updated to the tag's content, but nothing is deleted): they're also used by always-current,
// non-versioned components (e.g. src/components/PageFrame.astro) whose assets must keep
// resolving no matter which historical tag's docs are currently being archived.
const EXACT_CONTENT_PATHS = ['docs/src/content/docs'];
const ADDITIVE_CONTENT_PATHS = ['docs/src/assets', 'docs/public'];

function git(args, { checkExists = false } = {}) {
	return execFileSync('git', args, {
		cwd: repoRoot,
		encoding: 'utf8',
		stdio: ['ignore', 'pipe', checkExists ? 'ignore' : 'pipe'],
	}).trim();
}

function pathExistsAt(rev, repoRelativePath) {
	try {
		git(['cat-file', '-e', `${rev}:${repoRelativePath}`], { checkExists: true });
		return true;
	} catch {
		return false;
	}
}

function listTree(rev, repoRelativePath) {
	if (!pathExistsAt(rev, repoRelativePath)) return [];
	const out = git(['ls-tree', '-r', '--name-only', rev, '--', repoRelativePath]);
	return out ? out.split('\n').filter(Boolean) : [];
}

function compareXY(a, b) {
	const [aMajor, aMinor] = a.split('.').map(Number);
	const [bMajor, bMinor] = b.split('.').map(Number);
	return aMajor - bMajor || aMinor - bMinor;
}

function computeDesiredVersions() {
	const tags = git(['tag', '-l', 'v*', '--sort=v:refname']).split('\n').filter(Boolean);
	const desired = new Map(); // slug ("X.Y") -> tag, ascending order so the last write is the latest patch
	for (const tag of tags) {
		const match = /^v(\d+\.\d+)\.\d+$/.exec(tag);
		if (!match) continue;
		const [, slug] = match;
		if (!pathExistsAt(tag, 'docs/src/content/docs')) continue; // tag predates the docs site
		desired.set(slug, tag);
	}
	return desired;
}

function loadCurrentVersions() {
	if (!existsSync(versionsFile)) return new Map();
	return new Map(Object.entries(JSON.parse(readFileSync(versionsFile, 'utf8'))));
}

function saveCurrentVersions(map) {
	const sortedEntries = [...map.entries()].sort(([a], [b]) => compareXY(a, b));
	writeFileSync(versionsFile, `${JSON.stringify(Object.fromEntries(sortedEntries), null, 2)}\n`);
}

// Makes a path in the working tree match `rev`, removing files that were present at `fromRev`
// but are absent from `rev`. Only ever touches files tracked by git at `fromRev` or `rev`, so
// untracked generated version snapshots (e.g. src/content/docs/1.0/) are left alone.
function overlayExact(fromRev, rev, repoRelativePath) {
	const before = new Set(listTree(fromRev, repoRelativePath));
	const target = listTree(rev, repoRelativePath);
	const targetSet = new Set(target);
	for (const file of before) {
		if (!targetSet.has(file)) rmSync(path.join(repoRoot, file), { force: true });
	}
	if (target.length > 0) git(['checkout', rev, '--', repoRelativePath]);
}

// Updates files in a path to `rev`'s content without deleting anything absent from `rev`.
function overlayAdditive(rev, repoRelativePath) {
	if (listTree(rev, repoRelativePath).length > 0) git(['checkout', rev, '--', repoRelativePath]);
}

// Archives `rev`'s docs content into the working tree, moving from whatever revision it
// currently reflects (`fromRev`). Versioned page content is replaced exactly. Assets/public are
// only ever added to/updated, never pruned, while `rev` is a historical tag: they're also used
// by always-current, non-versioned components (e.g. src/components/PageFrame.astro) whose
// assets must keep resolving no matter which historical tag's docs are being archived. When
// restoring the live "Latest" state (`rev === 'HEAD'`), assets/public are also replaced exactly.
function overlayContent(fromRev, rev) {
	for (const repoRelativePath of EXACT_CONTENT_PATHS) {
		overlayExact(fromRev, rev, repoRelativePath);
	}
	for (const repoRelativePath of ADDITIVE_CONTENT_PATHS) {
		if (rev === 'HEAD') overlayExact(fromRev, rev, repoRelativePath);
		else overlayAdditive(rev, repoRelativePath);
	}
}

function removeArchivedSlug(slug) {
	rmSync(path.join(docsRoot, 'src/content/docs', slug), { recursive: true, force: true });
	rmSync(path.join(docsRoot, 'src/content/versions', `${slug}.json`), { force: true });
}

function build() {
	execFileSync('npm', ['run', 'build'], { cwd: docsRoot, stdio: 'inherit' });
}

function main() {
	const desired = computeDesiredVersions();
	const current = loadCurrentVersions();

	const outdated = [...desired.entries()]
		.filter(([slug, tag]) => current.get(slug) !== tag)
		.sort(([a], [b]) => compareXY(a, b));

	if (outdated.length === 0) {
		console.log('All documentation versions are already up to date with their release tags.');
		return;
	}

	let overlayRev = 'HEAD';

	for (const [slug, tag] of outdated) {
		console.log(`Archiving docs version ${slug} from tag ${tag}...`);
		if (current.has(slug)) removeArchivedSlug(slug);

		overlayContent(overlayRev, tag);
		overlayRev = tag;

		current.set(slug, tag);
		saveCurrentVersions(current);

		build();
	}

	overlayContent(overlayRev, 'HEAD');
	console.log('Version sync complete:', Object.fromEntries(current));
}

main();
