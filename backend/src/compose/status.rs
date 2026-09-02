//! Runtime status of a stack, derived from Docker rather than stored.
//!
//! Spec §4.2: there is no stack-state database. Every status in the UI comes
//! from `docker compose ps` on the stack directory.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackStatus {
    /// Every service container is up (and not reporting unhealthy).
    Running,
    /// Some containers are up, others are not.
    Partial,
    /// Containers exist but none is running.
    Stopped,
    /// Compose knows the project but no container has been created yet.
    NotCreated,
    /// `docker compose ps` failed, or the stack is ambiguous.
    Unknown,
}

/// One container as reported by `docker compose ps --format json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServiceStatus {
    #[serde(default, rename = "Service")]
    pub service: String,
    #[serde(default, rename = "Name")]
    pub name: String,
    #[serde(default, rename = "State")]
    pub state: String,
    #[serde(default, rename = "Status")]
    pub status: String,
    #[serde(default, rename = "Health")]
    pub health: String,
}

impl ServiceStatus {
    /// A container counts as up when Docker says it is running and, if it
    /// declares a healthcheck, that check is not failing.
    pub fn is_up(&self) -> bool {
        self.state.eq_ignore_ascii_case("running") && !self.health.eq_ignore_ascii_case("unhealthy")
    }
}

/// Parses `docker compose ps --format json` output.
///
/// Compose has shipped both shapes: a JSON array (v2.21+) and one JSON object
/// per line before that. Both are accepted so the bundled CLI version is not
/// baked into the parser.
pub fn parse_ps(stdout: &str) -> Result<Vec<ServiceStatus>, serde_json::Error> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed);
    }
    let mut services = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        services.push(serde_json::from_str(line)?);
    }
    Ok(services)
}

/// Collapses per-container state into the single status shown on the card.
pub fn derive(services: &[ServiceStatus]) -> StackStatus {
    if services.is_empty() {
        return StackStatus::NotCreated;
    }
    let up = services.iter().filter(|s| s.is_up()).count();
    match up {
        0 => StackStatus::Stopped,
        n if n == services.len() => StackStatus::Running,
        _ => StackStatus::Partial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARRAY: &str = r#"[
      {"Service":"web","Name":"app-web-1","State":"running","Status":"Up 2 hours","Health":""},
      {"Service":"db","Name":"app-db-1","State":"running","Status":"Up 2 hours","Health":"healthy"}
    ]"#;

    const NDJSON: &str = concat!(
        r#"{"Service":"web","Name":"app-web-1","State":"running","Status":"Up","Health":""}"#,
        "\n",
        r#"{"Service":"db","Name":"app-db-1","State":"exited","Status":"Exited (0)","Health":""}"#,
    );

    #[test]
    fn parses_the_json_array_shape() {
        let services = parse_ps(ARRAY).unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(derive(&services), StackStatus::Running);
    }

    #[test]
    fn parses_the_ndjson_shape() {
        let services = parse_ps(NDJSON).unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(derive(&services), StackStatus::Partial);
    }

    #[test]
    fn empty_output_means_nothing_was_created() {
        assert_eq!(parse_ps("").unwrap().len(), 0);
        assert_eq!(parse_ps("  \n ").unwrap().len(), 0);
        assert_eq!(derive(&[]), StackStatus::NotCreated);
    }

    #[test]
    fn all_containers_down_is_stopped() {
        let services = parse_ps(
            r#"[{"Service":"web","Name":"n","State":"exited","Status":"Exited (0)","Health":""}]"#,
        )
        .unwrap();
        assert_eq!(derive(&services), StackStatus::Stopped);
    }

    #[test]
    fn created_but_never_started_is_stopped() {
        let services = parse_ps(
            r#"[{"Service":"web","Name":"n","State":"created","Status":"Created","Health":""}]"#,
        )
        .unwrap();
        assert_eq!(derive(&services), StackStatus::Stopped);
    }

    #[test]
    fn an_unhealthy_container_does_not_count_as_up() {
        let services = parse_ps(
            r#"[{"Service":"web","Name":"n","State":"running","Status":"Up","Health":"unhealthy"}]"#,
        )
        .unwrap();
        assert!(!services[0].is_up());
        assert_eq!(derive(&services), StackStatus::Stopped);
    }

    #[test]
    fn a_starting_healthcheck_still_counts_as_up() {
        let services = parse_ps(
            r#"[{"Service":"web","Name":"n","State":"running","Status":"Up","Health":"starting"}]"#,
        )
        .unwrap();
        assert_eq!(derive(&services), StackStatus::Running);
    }

    #[test]
    fn unknown_fields_do_not_break_parsing() {
        let services =
            parse_ps(r#"[{"Service":"web","State":"running","Publishers":[{"URL":"0.0.0.0"}]}]"#)
                .unwrap();
        assert_eq!(services[0].service, "web");
        assert_eq!(services[0].health, "");
    }

    #[test]
    fn malformed_output_is_an_error_not_a_silent_empty_list() {
        assert!(parse_ps("not json").is_err());
    }
}
