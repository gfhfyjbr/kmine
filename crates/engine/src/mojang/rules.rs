use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeatureSet {
    pub demo: bool,
    pub custom_resolution: bool,
    pub quick_play_single: bool,
    pub quick_play_multi: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleOs {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuleFeatures {
    pub is_demo_user: Option<bool>,
    pub has_custom_resolution: Option<bool>,
    pub has_quick_plays_support: Option<bool>,
    pub is_quick_play_singleplayer: Option<bool>,
    pub is_quick_play_multiplayer: Option<bool>,
    pub is_quick_play_realms: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub action: RuleAction,
    pub os: Option<RuleOs>,
    pub features: Option<RuleFeatures>,
}

pub fn current_os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        std::env::consts::OS
    }
}

pub fn current_os_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "x86",
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        other => other,
    }
}

pub fn rule_allows(rules: &[Rule], features: &FeatureSet) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, features) {
            allowed = matches!(rule.action, RuleAction::Allow);
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, features: &FeatureSet) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != current_os_name() {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != current_os_arch() {
                return false;
            }
        }
    }
    if let Some(req) = &rule.features {
        if !features_match(req, features) {
            return false;
        }
    }
    true
}

fn features_match(req: &RuleFeatures, set: &FeatureSet) -> bool {
    feature_ok(req.is_demo_user, set.demo)
        && feature_ok(req.has_custom_resolution, set.custom_resolution)
        && feature_ok(req.is_quick_play_singleplayer, set.quick_play_single)
        && feature_ok(req.is_quick_play_multiplayer, set.quick_play_multi)
        && feature_ok(req.has_quick_plays_support, false)
        && feature_ok(req.is_quick_play_realms, false)
}

fn feature_ok(requested: Option<bool>, actual: bool) -> bool {
    requested.is_none_or(|want| want == actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rules_allow() {
        assert!(rule_allows(&[], &FeatureSet::default()));
    }

    #[test]
    fn unmatched_os_rule_disallows() {
        let rules = [Rule {
            action: RuleAction::Allow,
            os: Some(RuleOs {
                name: Some("not-a-real-os".into()),
                arch: None,
                version: None,
            }),
            features: None,
        }];
        assert!(!rule_allows(&rules, &FeatureSet::default()));
    }

    #[test]
    fn last_matching_rule_wins() {
        let rules = [
            Rule {
                action: RuleAction::Allow,
                os: None,
                features: None,
            },
            Rule {
                action: RuleAction::Disallow,
                os: None,
                features: None,
            },
        ];
        assert!(!rule_allows(&rules, &FeatureSet::default()));
    }

    #[test]
    fn demo_feature_must_be_true() {
        let rules = [Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some(RuleFeatures {
                is_demo_user: Some(true),
                ..Default::default()
            }),
        }];
        let mut feat = FeatureSet::default();
        assert!(!rule_allows(&rules, &feat));
        feat.demo = true;
        assert!(rule_allows(&rules, &feat));
    }

    #[test]
    fn current_os_name_is_mojang() {
        let name = current_os_name();
        #[cfg(target_os = "macos")]
        assert_eq!(name, "osx");
        #[cfg(target_os = "linux")]
        assert_eq!(name, "linux");
        #[cfg(target_os = "windows")]
        assert_eq!(name, "windows");
        assert!(matches!(name, "osx" | "linux" | "windows"));
    }

    #[test]
    fn current_os_arch_is_mojang() {
        let arch = current_os_arch();
        #[cfg(target_arch = "aarch64")]
        assert_eq!(arch, "arm64");
        #[cfg(target_arch = "x86_64")]
        assert_eq!(arch, "x86_64");
        #[cfg(target_arch = "x86")]
        assert_eq!(arch, "x86");
        assert!(matches!(arch, "x86" | "x86_64" | "arm64") || !arch.is_empty());
    }
}
