// #![allow(clippy::disallowed_macros)]
// use serde_yaml::Value;
// use serial_test::serial;
// use std::env;
// use std::fs;
// use tempfile::tempdir;
// use s4n::commands::*;

// const CWL_CONTENT: &str = r"
//     class: CommandLineTool
//     baseCommand: echo
//     hints:
//       DockerRequirement:
//         dockerPull: node:slim
//     inputs: []
//     outputs: []
//     ";

// const CWL_CONTENT_ANNOTATED: &str = r#"
//     class: CommandLineTool
//     baseCommand: echo
//     hints:
//       DockerRequirement:
//       dockerPull: node:slim
//     inputs: []
//     outputs: []
//     s:author:
//     - class: s:Person
//       s:identifier: https://orcid.org/0000-0002-6130-1021
//       s:email: mailto:dyuen@oicr.on.ca
//       s:name: Denis Yuen
//     s:contributor:
//     - class: s:Person
//       s:identifier: https://orcid.org/0000-0002-6130-1021
//       s:email: mailto:dyuen@oicr.on.ca
//       s:name: Denis Yuen
//     arc:performer:
//     - class: arc:Person
//       arc:first name: Jane
//       arc:last name: Doe2
//       arc:email: jdoe@mail.de
//       arc:affiliation: institution
//       arc:has role:
//       - class: arc:role
//         arc:term accession: http://purl.obolibrary.org/obo/NCIT_C170397
//         arc:annotation value: Formal Search
//     s:citation: https://dx.doi.org/10.6084/m9.figshare.3115156.v2
//     s:codeRepository: https://github.com/common-workflow-language/common-workflow-language
//     s:dateCreated: "2016-12-13"
//     s:license: https://spdx.org/licenses/Apache-2.0
//     $namespaces:
//       s: https://schema.org/
//       arc: https://github.com/nfdi4plants/ARC_ontology
//     $schemas:
//       - https://schema.org/version/latest/schemaorg-current-https.rdf
//       - https://raw.githubusercontent.com/nfdi4plants/ARC_ontology/main/ARC_v2.0.owl
//     "#;

// #[tokio::test]
// #[serial]
// async fn test_annotate_name() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();

//     env::set_current_dir(dir.path()).unwrap();

//     let temp_file_name = "test.cwl";

//     fs::write(temp_file_name, CWL_CONTENT).expect("Failed to write CWL file");

//     let command = AnnotateCommands::Name {
//         cwl_name: temp_file_name.to_string(),
//         name: "MyWorkflow".to_string(),
//     };

//     let result = handle_annotate_commands(&command).await;

//     assert!(result.is_ok(), "Expected Ok(()), got {result:?}");

//     let updated_content = fs::read_to_string(temp_file_name).expect("Failed to read updated CWL file");
//     assert!(
//         updated_content.contains("MyWorkflow"),
//         "Expected name annotation to be added, but got: {updated_content}"
//     );

//     env::set_current_dir(current).unwrap();
// }

// #[tokio::test]
// #[serial]
// async fn test_annotate_description() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();

//     env::set_current_dir(dir.path()).unwrap();

//     let temp_file_name = "test.cwl";

//     fs::write(temp_file_name, CWL_CONTENT).expect("Failed to write CWL file");

//     let command = AnnotateCommands::Description {
//         cwl_name: temp_file_name.to_string(),
//         description: "MyWorkflow description".to_string(),
//     };

//     let result = handle_annotate_commands(&command).await;

//     assert!(result.is_ok(), "Expected Ok(()), got {result:?}");

//     let updated_content = fs::read_to_string(temp_file_name).expect("Failed to read updated CWL file");
//     assert!(
//         updated_content.contains("MyWorkflow description"),
//         "Expected description annotation to be added, but got: {updated_content}"
//     );

//     env::set_current_dir(current).unwrap();
// }

// #[tokio::test]
// #[serial]
// async fn test_annotate_performer() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();

//     env::set_current_dir(dir.path()).unwrap();

//     let temp_file_name = "test.cwl";

//     fs::write(temp_file_name, CWL_CONTENT).expect("Failed to write CWL file");

//     let command = AnnotateCommands::Performer(PerformerArgs {
//         cwl_name: temp_file_name.to_string(),
//         first_name: Some("J".to_string()),
//         last_name: Some("Doe".to_string()),
//         mail: Some("doe@mail.com".to_string()),
//         affiliation: Some("institute1".to_string()),
//         role: None,
//         address: None,
//         mid_initials: None,
//         phone: None,
//         fax: None,
//     });
//     let result = handle_annotate_commands(&command).await;

//     assert!(result.is_ok(), "Expected Ok(()), got {result:?}");

//     let updated_content = fs::read_to_string(temp_file_name).expect("Failed to read updated CWL file");
//     assert!(
//         updated_content.contains("arc:first name: J")
//             && updated_content.contains("arc:last name: Doe")
//             && updated_content.contains("arc:email: doe@mail.com")
//             && updated_content.contains("arc:affiliation: institute1"),
//         "Expected performer annotation to be added, but got: {updated_content}"
//     );

//     env::set_current_dir(current).unwrap();
// }

// #[tokio::test]
// #[serial]
// async fn test_annotate_process_step_with_input_output() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();
//     env::set_current_dir(dir.path()).unwrap();

//     let cwl_file_name = "test_process.cwl";

//     fs::write(cwl_file_name, CWL_CONTENT).expect("Failed to write CWL file");

//     let args = AnnotateCommands::Process(AnnotateProcessArgs {
//         cwl_name: cwl_file_name.to_string(),
//         name: "sequence1".to_string(),
//         input: Some("input_data".to_string()),
//         output: Some("output_data".to_string()),
//         parameter: None,
//         value: None,
//     });

//     let result = handle_annotate_commands(&args).await;

//     assert!(result.is_ok(), "Expected Ok(()), got {result:?}");

//     let updated_content = fs::read_to_string(cwl_file_name).expect("Failed to read updated CWL file");
//     println!("updated_content {updated_content:?}");
//     assert!(updated_content.contains("arc:has process sequence"), "Process sequence not added");
//     assert!(updated_content.contains("arc:name: sequence1"), "Name not added");
//     assert!(updated_content.contains("arc:has input"), "has input not added");
//     assert!(updated_content.contains("arc:has output"), "has output not added");
//     assert!(updated_content.contains("input_data"), "Input not added");
//     assert!(updated_content.contains("output_data"), "Output not added");

//     env::set_current_dir(current).unwrap();
// }

// #[tokio::test]
// #[serial]
// async fn test_annotate_process() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();
//     env::set_current_dir(dir.path()).unwrap();

//     let cwl_file_name = "test_process.cwl";

//     fs::write(cwl_file_name, CWL_CONTENT).expect("Failed to write CWL file");

//     let args = AnnotateCommands::Process(AnnotateProcessArgs {
//         cwl_name: cwl_file_name.to_string(),
//         name: "sequence1".to_string(),
//         input: Some("input_data".to_string()),
//         output: Some("output_data".to_string()),
//         parameter: None,
//         value: None,
//     });

//     let result =  handle_annotate_commands(&args).await;

//     assert!(result.is_ok(), "Expected Ok(()), got {result:?}");

//     let updated_content = fs::read_to_string(cwl_file_name).expect("Failed to read updated CWL file");
//     println!("updated_content {updated_content:?}");
//     assert!(updated_content.contains("arc:has process sequence"), "Process sequence not added");
//     assert!(updated_content.contains("arc:name: sequence1"), "Name not added");
//     assert!(updated_content.contains("arc:has input"), "has input not added");
//     assert!(updated_content.contains("arc:has output"), "has output not added");
//     assert!(updated_content.contains("input_data"), "Input not added");
//     assert!(updated_content.contains("output_data"), "Output not added");

//     env::set_current_dir(current).unwrap();
// }

// #[tokio::test]
// #[serial]
// async fn test_annotate_performer_add_to_existing_list() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();
//     env::set_current_dir(dir.path()).unwrap();

//     let cwl_filename = "test_process.cwl";

//     fs::write(cwl_filename, CWL_CONTENT_ANNOTATED).expect("Failed to write CWL file");

//     let args = AnnotateCommands::Performer(PerformerArgs {
//         cwl_name: cwl_filename.to_string(),
//         first_name: Some("Jane".to_string()),
//         last_name: Some("Smith".to_string()),
//         mail: Some("jane.smith@example.com".to_string()),
//         affiliation: Some("Example Organization".to_string()),
//         role: None,
//         address: None,
//         mid_initials: None,
//         phone: None,
//         fax: None,
//     });

//     let result =  handle_annotate_commands(&args).await;
//     assert!(result.is_ok(), "annotate_performer failed");

//     let updated_content = fs::read_to_string(cwl_filename).expect("Failed to read updated CWL file");

//     assert!(updated_content.contains("Jane"), "First name not added");
//     assert!(updated_content.contains("Smith"), "Last name not added");
//     assert!(updated_content.contains("jane.smith@example.com"), "Email not added");
//     assert!(updated_content.contains("Example Organization"), "Affiliation not added");

//     env::set_current_dir(current).unwrap();
// }

// #[tokio::test]
// #[serial]
// async fn test_annotate_performer_avoid_duplicate() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();
//     env::set_current_dir(dir.path()).unwrap();

//     let cwl_content = r#"
//     arc:performer:
//       - class: arc:Person
//         arc:first name: "Charlie"
//         arc:last name: "Davis"
//         arc:email: "charlie.davis@example.com"
//     "#;

//     let cwl_filename = "test.cwl";

//     std::fs::write(cwl_filename, cwl_content).unwrap();

//     let args = AnnotateCommands::Performer(PerformerArgs {
//         cwl_name: cwl_filename.to_string(),
//         first_name: Some("Charlie".to_string()),
//         last_name: Some("Davis".to_string()),
//         mail: Some("charlie.davis@example.com".to_string()),
//         affiliation: None,
//         role: None,
//         address: None,
//         mid_initials: None,
//         phone: None,
//         fax: None,
//     });

//     let result =  handle_annotate_commands(&args).await;

//     assert!(result.is_ok(), "annotate_performer failed");

//     let updated_yaml: Value = serde_yaml::from_str(&std::fs::read_to_string(cwl_filename).unwrap()).unwrap();

//     if let Value::Sequence(performers) = &updated_yaml["arc:performer"] {
//         assert_eq!(performers.len(), 1, "Expected 1 performer, found {}", performers.len());
//     } else {
//         panic!("Expected 'arc:performer' to be a sequence.");
//     }

//     env::set_current_dir(current).unwrap();
// }

// #[tokio::test]
// #[serial]
// async fn test_annotate_performer_invalid_root() {
//     let dir = tempdir().unwrap();
//     let current = env::current_dir().unwrap();
//     env::set_current_dir(dir.path()).unwrap();

//     let cwl_content = r"
//     - not_a_mapping
//     ";

//     let cwl_filename = "test_invalid_root.cwl";

//     std::fs::write(cwl_filename, cwl_content).unwrap();

//     let args = AnnotateCommands::Performer(PerformerArgs {
//         cwl_name: cwl_filename.to_string(),
//         first_name: Some("David".to_string()),
//         last_name: Some("Evans".to_string()),
//         role: None,
//         address: None,
//         mid_initials: None,
//         phone: None,
//         fax: None,
//         affiliation: None,
//         mail: None
//     });

//     let result =  handle_annotate_commands(&args).await;

//     assert!(result.is_err(), "annotate_performer expected to fail");

//     env::set_current_dir(current).unwrap();
// }

