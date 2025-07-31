use std::path::{Path, PathBuf};

use rstest::rstest;
use winnow::{error::ContextError, Parser};
use ya::parse::yaml_stream;

#[rstest]
fn check_test_suite(
    #[base_dir = "testdata/yaml-test-suite/"]
    #[files("**/in.yaml")]
    #[dirs]
    case: PathBuf,
) {
    println!("{}", case.display());
    let case_dir: &Path = case.parent().unwrap();
    let case_name = case_dir.file_name().unwrap().to_string_lossy();
    let error_file = case_dir.join("error");
    if error_file.exists() {
        println!("case {case_name} expects error");
        check_error(&case);
    } else {
        println!("case {case_name} expects valid input");
        check_input(&case, &case_dir.join("test.event"));
    }
}

fn check_error(input: &Path) {
    let input = std::fs::read_to_string(input).unwrap();
    yaml_stream::<_, ContextError>
        .parse(ya::parse::input::Input::new(&input))
        .unwrap_err();
}

fn check_input(input: &Path, event: &Path) {
    let input = std::fs::read_to_string(input).unwrap();
    let stream = yaml_stream::<_, ContextError>
        .parse(ya::parse::input::Input::new(&input))
        .unwrap();
    // TODO: compare stream against the event.
}
