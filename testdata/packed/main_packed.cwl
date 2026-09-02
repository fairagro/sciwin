{
    "$graph": [
        {
            "class": "CommandLineTool",
            "requirements": [
                {
                    "class": "InitialWorkDirRequirement",
                    "listing": [
                        {
                            "entryname": "workflows/calculation/calculation.py",
                            "entry": "import pandas as pd\nimport argparse\n\nparser = argparse.ArgumentParser(prog=\"python calculation.py\", description=\"Calculates the percentage of speakers for each language\")\nparser.add_argument(\"-p\", \"--population\", required=True, help=\"Path to the population.csv File\")\nparser.add_argument(\"-s\", \"--speakers\", required=True, help=\"Path to the speakers.csv File\")\n\nargs = parser.parse_args()\n\ndf = pd.read_csv(args.population)\nsum = df[\"population\"].sum()\n\nprint(f\"Total population: {sum}\")\n\ndf = pd.read_csv(args.speakers)\ndf[\"percentage\"] = df[\"speakers\"] / sum * 100\n\ndf.to_csv(\"results.csv\")\nprint(df.head(10))"
                        }
                    ]
                },
                {
                    "class": "DockerRequirement",
                    "dockerPull": "pandas/pandas:pip-all"
                }
            ],
            "inputs": [
                {
                    "id": "#calculation.cwl/population",
                    "type": "File",
                    "default": {
                        "class": "File",
                        "location": "file:///mnt/m4.4_sciwin_client/testdata/hello_world/data/population.csv"
                    },
                    "inputBinding": {
                        "prefix": "--population"
                    }
                },
                {
                    "id": "#calculation.cwl/speakers",
                    "type": "File",
                    "default": {
                        "class": "File",
                        "location": "file:///mnt/m4.4_sciwin_client/testdata/hello_world/data/speakers_revised.csv"
                    },
                    "inputBinding": {
                        "prefix": "--speakers"
                    }
                }
            ],
            "outputs": [
                {
                    "id": "#calculation.cwl/results",
                    "type": "File",
                    "outputBinding": {
                        "glob": "results.csv"
                    }
                }
            ],
            "baseCommand": [
                "python",
                "workflows/calculation/calculation.py"
            ],
            "id": "#calculation.cwl"
        },
        {
            "class": "Workflow",
            "inputs": [
                {
                    "id": "#main/population",
                    "type": "File",
                    "default": {
                        "class": "File",
                        "location": "file:///mnt/m4.4_sciwin_client/testdata/hello_world/data/population.csv"
                    }
                },
                {
                    "id": "#main/speakers",
                    "type": "File",
                    "default": {
                        "class": "File",
                        "location": "file:///mnt/m4.4_sciwin_client/testdata/hello_world/data/speakers_revised.csv"
                    }
                }
            ],
            "outputs": [
                {
                    "id": "#main/out",
                    "type": "File",
                    "outputSource": "#main/plot/o_results"
                }
            ],
            "steps": [
                {
                    "id": "#main/plot",
                    "in": [
                        {
                            "id": "#main/plot/results",
                            "source": "#main/calculation/results"
                        }
                    ],
                    "run": "#plot.cwl",
                    "out": [
                        "#main/plot/o_results"
                    ]
                },
                {
                    "id": "#main/calculation",
                    "in": [
                        {
                            "id": "#main/calculation/population",
                            "source": "#main/population"
                        },
                        {
                            "id": "#main/calculation/speakers",
                            "source": "#main/speakers"
                        }
                    ],
                    "run": "#calculation.cwl",
                    "out": [
                        "#main/calculation/results"
                    ]
                }
            ],
            "id": "#main"
        },
        {
            "class": "CommandLineTool",
            "requirements": [
                {
                    "class": "InitialWorkDirRequirement",
                    "listing": [
                        {
                            "entryname": "workflows/plot/plot.py",
                            "entry": "import matplotlib\nfrom matplotlib import pyplot as plt\nimport pandas as pd\nimport argparse\nimport scienceplots\nassert scienceplots, 'scienceplots needed'\nplt.style.use(['science', 'no-latex'])\n\nparser = argparse.ArgumentParser(prog=\"python plot.py\", description=\"Plots the percentage of speakers for each language\")\nparser.add_argument(\"-r\", \"--results\", required=True, help=\"Path to the results.csv File\")\nargs = parser.parse_args()\n\ndf = pd.read_csv(args.results)\ncolors = matplotlib.colormaps['tab10'](range(len(df)))\n\nax = df.plot.bar(x='language', y='percentage', legend=False, title='Language Popularity', color=colors) \nax.yaxis.set_label_text('Percentage (%)')\nax.xaxis.set_label_text('')\n\nplt.savefig('results.svg')"
                        }
                    ]
                },
                {
                    "class": "DockerRequirement",
                    "dockerFile": "FROM python\nRUN pip install matplotlib==3.10.6 SciencePlots pandas",
                    "dockerImageId": "matplotlib"
                }
            ],
            "inputs": [
                {
                    "id": "#plot.cwl/results",
                    "type": "File",
                    "default": {
                        "class": "File",
                        "location": "file:///mnt/m4.4_sciwin_client/testdata/hello_world/results.csv"
                    },
                    "inputBinding": {
                        "prefix": "--results"
                    }
                }
            ],
            "outputs": [
                {
                    "id": "#plot.cwl/o_results",
                    "type": "File",
                    "outputBinding": {
                        "glob": "results.svg"
                    }
                }
            ],
            "baseCommand": [
                "python",
                "workflows/plot/plot.py"
            ],
            "id": "#plot.cwl"
        }
    ],
    "cwlVersion": "v1.2"
}