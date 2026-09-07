//! Parses `docker compose stats --no-stream --format json`.
//!
//! One container's live resource usage, exactly as Docker formats it — no
//! percentage parsing, no unit conversion. The values are already what an
//! operator would see running the command by hand, and re-deriving them
//! server-side would be a second copy of Docker's own formatting to keep in
//! sync with a version this project does not pin.

use serde::{Deserialize, Serialize};

/// One container's resource usage, as reported by `docker compose stats`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContainerStats {
    #[serde(default, rename = "Name")]
    pub name: String,
    #[serde(default, rename = "CPUPerc")]
    pub cpu_perc: String,
    #[serde(default, rename = "MemUsage")]
    pub mem_usage: String,
    #[serde(default, rename = "MemPerc")]
    pub mem_perc: String,
    #[serde(default, rename = "NetIO")]
    pub net_io: String,
    #[serde(default, rename = "BlockIO")]
    pub block_io: String,
    #[serde(default, rename = "PIDs")]
    pub pids: String,
}

/// Parses one JSON object per line, tolerating blank lines and a trailing
/// newline. A stack with no running containers prints nothing, which is an
/// empty list rather than an error.
pub fn parse_stats(stdout: &str) -> Result<Vec<ContainerStats>, serde_json::Error> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let mut stats = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        stats.push(serde_json::from_str(line)?);
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NDJSON: &str = concat!(
        r#"{"Name":"app-web-1","CPUPerc":"0.15%","MemUsage":"12MiB / 512MiB","MemPerc":"2.34%","NetIO":"1.2kB / 0B","BlockIO":"0B / 0B","PIDs":"3"}"#,
        "\n",
        r#"{"Name":"app-db-1","CPUPerc":"1.02%","MemUsage":"80MiB / 512MiB","MemPerc":"15.6%","NetIO":"648B / 90B","BlockIO":"12kB / 0B","PIDs":"11"}"#,
    );

    #[test]
    fn parses_one_object_per_line() {
        let stats = parse_stats(NDJSON).unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].name, "app-web-1");
        assert_eq!(stats[0].cpu_perc, "0.15%");
        assert_eq!(stats[1].mem_usage, "80MiB / 512MiB");
    }

    #[test]
    fn empty_output_means_no_running_containers() {
        assert_eq!(parse_stats("").unwrap().len(), 0);
        assert_eq!(parse_stats("  \n ").unwrap().len(), 0);
    }

    #[test]
    fn values_are_kept_verbatim_not_reparsed() {
        let stats = parse_stats(
            r#"{"Name":"n","CPUPerc":"0.00%","MemUsage":"1MiB / 1GiB","MemPerc":"0.10%","NetIO":"0B / 0B","BlockIO":"0B / 0B","PIDs":"1"}"#,
        )
        .unwrap();
        // Kept as Docker's own string, not split into a number and a unit.
        assert_eq!(stats[0].mem_usage, "1MiB / 1GiB");
    }

    #[test]
    fn unknown_fields_do_not_break_parsing() {
        let stats = parse_stats(
            r#"{"Name":"n","CPUPerc":"0.00%","ID":"abc123","Container":"abc123def456"}"#,
        )
        .unwrap();
        assert_eq!(stats[0].name, "n");
        assert_eq!(stats[0].mem_usage, "");
    }

    #[test]
    fn malformed_output_is_an_error_not_a_silent_empty_list() {
        assert!(parse_stats("not json").is_err());
    }
}
