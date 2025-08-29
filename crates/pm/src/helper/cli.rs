/// Parse script arguments from raw CLI args after -- separator.
///
/// Examples:
///   -- -e         => vec!["-e"]
///   -- -e -f      => vec!["-e", "-f"]
///   (no --)       => vec![] (no separator found)
///
/// Returns None if no -- separator is found.
pub fn parse_script_and_args(args: &[String]) -> Option<Vec<&str>> {
    if let Some(dashdash) = args.iter().position(|x| x == "--") {
        // All arguments after -- are script arguments
        let script_args = args[dashdash + 1..].iter().map(|s| s.as_str()).collect();
        Some(script_args)
    } else {
        // No -- separator found
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_script_with_dashdash() {
        let args = vec!["--".to_string(), "-e".to_string()];
        let result = parse_script_and_args(&args);
        assert_eq!(result, Some(vec!["-e"]));
    }

    #[test]
    fn test_parse_script_direct() {
        let args = vec!["--".to_string(), "-e".to_string()];
        let result = parse_script_and_args(&args);
        assert_eq!(result, Some(vec!["-e"]));
    }

    #[test]
    fn test_parse_script_with_cli_args() {
        let args = vec!["--".to_string(), "-e".to_string(), "-f".to_string()];
        let result = parse_script_and_args(&args);
        assert_eq!(result, Some(vec!["-e", "-f"]));
    }

    #[test]
    fn test_parse_script_without_dashdash() {
        let args = vec!["-a=b".to_string()];
        let result = parse_script_and_args(&args);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_script_with_run_command() {
        let args = vec!["--".to_string(), "-e".to_string(), "-f".to_string()];
        let result = parse_script_and_args(&args);
        assert_eq!(result, Some(vec!["-e", "-f"]));
    }

    #[test]
    fn test_no_script_name() {
        let args = vec!["--".to_string(), "-e".to_string()];
        let result = parse_script_and_args(&args);
        assert_eq!(result, Some(vec!["-e"]));
    }

    #[test]
    fn test_empty_args() {
        let args: Vec<String> = vec![];
        let result = parse_script_and_args(&args);
        assert_eq!(result, None);
    }
}
