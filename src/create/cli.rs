//! Shared helpers for create-compatible command-line frontends.

use std::path::{Path, PathBuf};

const SOURCE_LIST_REQUIRED: &str = "You must specify a list of files when creating.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedundancyOption {
    Percent(u32),
    TargetSize(u64),
}

/// Parse redundancy option as a percentage or target size.
///
/// Supports par2cmdline style `m40` and suffix style `40m`.
pub fn parse_redundancy_option(redundancy_str: &str) -> Result<RedundancyOption, String> {
    if redundancy_str.is_empty() {
        return Err("Invalid redundancy option".to_string());
    }

    let lower = redundancy_str.to_ascii_lowercase();
    let first = lower.as_bytes()[0] as char;
    let last = lower.as_bytes()[lower.len() - 1] as char;

    if matches!(first, 'g' | 'm' | 'k') {
        parse_redundancy_size(first, &lower[1..])
    } else if matches!(last, 'g' | 'm' | 'k') {
        parse_redundancy_size(last, &lower[..lower.len() - 1])
    } else {
        let percentage = redundancy_str
            .parse()
            .map_err(|_| "Invalid redundancy percentage".to_string())?;
        Ok(RedundancyOption::Percent(percentage))
    }
}

fn parse_redundancy_size(unit: char, digits: &str) -> Result<RedundancyOption, String> {
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("Invalid redundancy size value".to_string());
    }

    let mut bytes: u64 = digits
        .parse()
        .map_err(|_| "Invalid redundancy size value".to_string())?;
    bytes = match unit {
        'g' => bytes
            .checked_mul(1024 * 1024 * 1024)
            .ok_or_else(|| "Invalid redundancy size value".to_string())?,
        'm' => bytes
            .checked_mul(1024 * 1024)
            .ok_or_else(|| "Invalid redundancy size value".to_string())?,
        'k' => bytes
            .checked_mul(1024)
            .ok_or_else(|| "Invalid redundancy size value".to_string())?,
        _ => unreachable!("unit checked by caller"),
    };

    Ok(RedundancyOption::TargetSize(bytes))
}

pub fn expand_source_files(inputs: Vec<PathBuf>, recurse: bool) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() && recurse {
            collect_directory_files(&input, &mut files)?;
        } else {
            files.push(input);
        }
    }
    Ok(files)
}

pub fn resolve_create_inputs(
    par2_file: &str,
    archive_name: Option<&str>,
    source_inputs: Vec<PathBuf>,
    recurse: bool,
) -> Result<(String, Vec<PathBuf>), String> {
    if source_inputs.is_empty() {
        return resolve_implicit_source(par2_file, archive_name);
    }

    let output_name = archive_name.unwrap_or(par2_file).to_string();
    let source_files = expand_source_files(source_inputs, recurse)
        .map_err(|err| format!("Failed to expand source file list: {err}"))?;
    Ok((output_name, source_files))
}

fn resolve_implicit_source(
    par2_file: &str,
    archive_name: Option<&str>,
) -> Result<(String, Vec<PathBuf>), String> {
    let source = PathBuf::from(par2_file);
    let metadata = std::fs::metadata(&source).map_err(|_| SOURCE_LIST_REQUIRED.to_string())?;

    if par2_file.to_ascii_lowercase().ends_with(".par2")
        || !metadata.is_file()
        || metadata.len() == 0
    {
        return Err(SOURCE_LIST_REQUIRED.to_string());
    }

    let output_name = archive_name
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{par2_file}.par2"));
    Ok((output_name, vec![source]))
}

pub fn validate_recovery_file_count(count: u32) -> Result<u32, String> {
    if (1..=31).contains(&count) {
        Ok(count)
    } else {
        Err(format!(
            "Invalid recovery file count: {count} (must be 1-31)"
        ))
    }
}

pub fn warn_for_high_redundancy(redundancy: Option<RedundancyOption>) {
    if let Some(RedundancyOption::Percent(percent)) = redundancy {
        if percent > 100 {
            eprintln!("WARNING: Creating recovery file(s) with {percent}% redundancy.");
        }
    }
}

fn collect_directory_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_directory_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_redundancy_percent() {
        assert_eq!(
            parse_redundancy_option("10").unwrap(),
            RedundancyOption::Percent(10)
        );
    }

    #[test]
    fn parse_redundancy_target_size_prefix_and_suffix() {
        assert_eq!(
            parse_redundancy_option("k10").unwrap(),
            RedundancyOption::TargetSize(10 * 1024)
        );
        assert_eq!(
            parse_redundancy_option("10m").unwrap(),
            RedundancyOption::TargetSize(10 * 1024 * 1024)
        );
        assert_eq!(
            parse_redundancy_option("g2").unwrap(),
            RedundancyOption::TargetSize(2 * 1024 * 1024 * 1024)
        );
    }

    #[test]
    fn parse_redundancy_rejects_invalid_target_size() {
        assert!(parse_redundancy_option("m").is_err());
        assert!(parse_redundancy_option("m1x").is_err());
        assert!(parse_redundancy_option("").is_err());
    }

    #[test]
    fn expand_source_files_recurses_in_stable_order() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("b.txt"), b"b").unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();

        let files = expand_source_files(vec![root.clone()], true).unwrap();
        assert_eq!(files, vec![root.join("a.txt"), nested.join("b.txt")]);

        let files = expand_source_files(vec![root.clone()], false).unwrap();
        assert_eq!(files, vec![root]);
    }

    #[test]
    fn validate_recovery_file_count_matches_turbo_limit() {
        assert_eq!(validate_recovery_file_count(1).unwrap(), 1);
        assert_eq!(validate_recovery_file_count(31).unwrap(), 31);
        assert!(validate_recovery_file_count(0).is_err());
        assert!(validate_recovery_file_count(32).is_err());
    }
}
