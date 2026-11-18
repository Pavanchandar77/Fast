use nx_governor_policy::{
    matches_rule, GovernorRules, Pid, CRITICAL_SYSTEM_PROCESSES, KNOWN_GAME_EXECUTABLES,
    NEXUS_CORE_PROCESSES,
};

#[test]
fn test_exact_match_no_substring_false_positive_cl_clean() {
    // "cl.exe" must NOT match "clean.exe"
    assert!(!matches_rule("clean.exe", "cl.exe"));
    assert!(!matches_rule("clean", "cl.exe"));
    assert!(!matches_rule("clean.exe", "cl"));
}

#[test]
fn test_exact_match_no_substring_false_positive_cl_client() {
    // "cl.exe" must NOT match "client.exe"
    assert!(!matches_rule("client.exe", "cl.exe"));
    assert!(!matches_rule("client", "cl.exe"));
}

#[test]
fn test_exact_match_no_substring_false_positive_cl_eclipse() {
    // "cl.exe" must NOT match "eclipse.exe"
    assert!(!matches_rule("eclipse.exe", "cl.exe"));
    assert!(!matches_rule("eclipse", "cl"));
}

#[test]
fn test_exact_match_no_substring_false_positive_rustc_trustcenter() {
    // "rustc.exe" must NOT match "trustcenter.exe"
    assert!(!matches_rule("trustcenter.exe", "rustc.exe"));
    assert!(!matches_rule("trustcenter", "rustc"));
}

#[test]
fn test_exact_match_no_substring_false_positive_app_apple() {
    // "app.exe" must NOT match "apple.exe"
    assert!(!matches_rule("apple.exe", "app.exe"));
    assert!(!matches_rule("apple", "app"));
}

#[test]
fn test_exact_match_stem_and_extension_variants() {
    // Exact stem match
    assert!(matches_rule("spotify.exe", "spotify.exe"));
    assert!(matches_rule("spotify", "spotify.exe"));
    assert!(matches_rule("spotify.exe", "spotify"));
    assert!(matches_rule("spotify", "spotify"));
}

#[test]
fn test_case_insensitivity() {
    assert!(matches_rule("SPOTIFY.EXE", "spotify.exe"));
    assert!(matches_rule("Discord.exe", "DISCORD.EXE"));
    assert!(matches_rule("CaRgO.eXe", "cargo"));
}

#[test]
fn test_path_normalization() {
    assert!(matches_rule(
        "C:\\Program Files\\Spotify\\Spotify.exe",
        "spotify.exe"
    ));
    assert!(matches_rule("/usr/bin/cargo", "cargo.exe"));
    assert!(matches_rule("D:\\Games\\Steam\\steam.exe", "steam"));
}

#[test]
fn test_wildcard_prefix_matching() {
    assert!(matches_rule("nexus-governor.exe", "nexus-*"));
    assert!(matches_rule("nexus-os.exe", "nexus-*"));
    assert!(matches_rule("nexus-vfs.exe", "nexus-*.exe"));
    // Negative cases
    assert!(!matches_rule("other-nexus.exe", "nexus-*"));
    assert!(!matches_rule("my-nexus-app.exe", "nexus-*"));
}

#[test]
fn test_wildcard_suffix_matching() {
    assert!(matches_rule("mygame.exe", "*game.exe"));
    assert!(matches_rule("coolgame.exe", "*game.exe"));
    assert!(!matches_rule("game.dll", "*game.exe"));
    assert!(!matches_rule("mygame.txt", "*game.exe"));
}

#[test]
fn test_critical_processes_classification() {
    let rules = GovernorRules::default();
    assert!(rules.is_critical(Pid(0), "idle"));
    assert!(rules.is_critical(Pid(4), "system"));

    for &crit in CRITICAL_SYSTEM_PROCESSES {
        assert!(
            rules.is_critical(Pid(100), crit),
            "Expected {} to be critical",
            crit
        );
    }

    for &nexus in NEXUS_CORE_PROCESSES {
        assert!(
            rules.is_critical(Pid(101), nexus),
            "Expected {} to be critical",
            nexus
        );
    }

    // Normal apps are not critical
    assert!(!rules.is_critical(Pid(500), "chrome.exe"));
    assert!(!rules.is_critical(Pid(501), "notepad.exe"));
}

#[test]
fn test_known_games_classification() {
    let rules = GovernorRules::default();
    for &game in KNOWN_GAME_EXECUTABLES {
        assert!(
            rules.is_known_game(game),
            "Expected {} to be classified as game",
            game
        );
    }
    assert!(!rules.is_known_game("excel.exe"));
    assert!(!rules.is_known_game("slack.exe"));
}

#[test]
fn test_high_priority_whitelist() {
    let rules = GovernorRules::new(
        &[] as &[&str],
        &["ableton.exe", "reaper.exe", "vrserver.exe"],
    );
    assert!(rules.is_high_priority_whitelisted("ableton.exe"));
    assert!(rules.is_high_priority_whitelisted("reaper.exe"));
    assert!(rules.is_high_priority_whitelisted("vrserver.exe"));
    assert!(!rules.is_high_priority_whitelisted("chrome.exe"));
    assert!(!rules.is_high_priority_whitelisted("game.exe"));
}

#[test]
fn test_runtime_rules_addition() {
    let mut rules = GovernorRules::default();
    assert!(!rules.is_excluded(Pid(700), "customapp.exe"));

    rules.add_exclude_rule("customapp.exe");
    assert!(rules.is_excluded(Pid(700), "customapp.exe"));

    assert!(!rules.is_high_priority_whitelisted("pro_audio.exe"));
    rules.add_high_priority_whitelist("pro_audio.exe");
    assert!(rules.is_high_priority_whitelisted("pro_audio.exe"));
}
