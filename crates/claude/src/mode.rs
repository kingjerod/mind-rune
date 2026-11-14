use mr_agent::PermissionMode;

/// Reads a permission mode name as `claude` writes it. Unknown names are kept.
pub fn mode_from_wire(name: &str) -> PermissionMode {
    match name {
        "default" | "manual" => PermissionMode::Ask,
        "acceptEdits" => PermissionMode::AcceptEdits,
        "plan" => PermissionMode::Plan,
        // Read from the program (2.1.323) and its documented `--permission-mode` values; not
        // yet seen on the wire. The live test prints whether `claude` accepts it.
        "auto" => PermissionMode::Auto,
        "bypassPermissions" => PermissionMode::Bypass,
        other => PermissionMode::Other(other.to_string()),
    }
}

/// The name `claude` uses for a permission mode.
pub fn mode_to_wire(mode: &PermissionMode) -> String {
    match mode {
        PermissionMode::Ask => "default".to_string(),
        PermissionMode::AcceptEdits => "acceptEdits".to_string(),
        PermissionMode::Plan => "plan".to_string(),
        PermissionMode::Auto => "auto".to_string(),
        PermissionMode::Bypass => "bypassPermissions".to_string(),
        PermissionMode::Other(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_names_map_both_ways_and_keep_unknown_ones() {
        assert_eq!(mode_from_wire("default"), PermissionMode::Ask);
        assert_eq!(mode_from_wire("manual"), PermissionMode::Ask);
        assert_eq!(mode_from_wire("acceptEdits"), PermissionMode::AcceptEdits);
        assert_eq!(mode_from_wire("plan"), PermissionMode::Plan);
        assert_eq!(mode_from_wire("bypassPermissions"), PermissionMode::Bypass);
        assert_eq!(
            mode_from_wire("dontAsk"),
            PermissionMode::Other("dontAsk".into())
        );
        assert_eq!(mode_from_wire("auto"), PermissionMode::Auto);
        assert_eq!(mode_to_wire(&PermissionMode::Auto), "auto");
        assert_eq!(mode_to_wire(&PermissionMode::Ask), "default");
        assert_eq!(
            mode_to_wire(&PermissionMode::Other("dontAsk".into())),
            "dontAsk"
        );
    }
}
