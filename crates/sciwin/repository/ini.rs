use configparser::ini::Ini;
use std::path::Path;

/// Removes a section from an INI file.
pub(crate) fn remove_section<P: AsRef<Path>>(
    file: P,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Ini::new();
    config.load(&file)?;
    config.remove_section(name);
    config.write(&file)?;
    Ok(())
}
