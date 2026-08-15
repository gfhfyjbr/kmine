use super::rules::{FeatureSet, rule_allows};
use super::{ArgValue, LaunchArgument, VersionInfo};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ArgContext {
    pub auth_player_name: String,
    pub auth_uuid: String,
    pub auth_access_token: String,
    pub user_type: String,
    pub version_name: String,
    pub version_type: String,
    pub client_id: String,
    pub auth_xuid: String,
    pub user_properties: String,
    pub classpath_separator: String,
    pub game_directory: String,
    pub assets_root: String,
    pub assets_index_name: String,
    pub natives_directory: String,
    pub launcher_name: String,
    pub launcher_version: String,
    pub classpath: String,
    pub library_directory: String,
    pub resolution_width: String,
    pub resolution_height: String,
    pub quick_play_singleplayer: Option<String>,
    pub quick_play_multiplayer: Option<String>,
}

const LEGACY_JVM: &[&str] = &[
    "-Djava.library.path=${natives_directory}",
    "-cp",
    "${classpath}",
];

pub fn join_classpath(entries: &[PathBuf]) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    entries
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join(sep)
}

pub fn interpolate(arg: &str, ctx: &ArgContext) -> String {
    let mut result = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match lookup(ctx, key) {
                    Some(val) => result.push_str(val),
                    None => {
                        result.push_str("${");
                        result.push_str(key);
                        result.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                result.push_str(&rest[start..]);
                return result;
            }
        }
    }
    result.push_str(rest);
    result
}

fn lookup<'a>(ctx: &'a ArgContext, key: &str) -> Option<&'a str> {
    match key {
        "auth_player_name" => Some(&ctx.auth_player_name),
        "auth_uuid" => Some(&ctx.auth_uuid),
        "auth_access_token" => Some(&ctx.auth_access_token),
        "user_type" => Some(&ctx.user_type),
        "version_name" => Some(&ctx.version_name),
        "version_type" => Some(&ctx.version_type),
        "clientid" => Some(&ctx.client_id),
        "auth_xuid" => Some(&ctx.auth_xuid),
        "user_properties" => Some(&ctx.user_properties),
        "classpath_separator" => Some(&ctx.classpath_separator),
        "game_directory" => Some(&ctx.game_directory),
        "assets_root" => Some(&ctx.assets_root),
        "assets_index_name" => Some(&ctx.assets_index_name),
        "natives_directory" => Some(&ctx.natives_directory),
        "launcher_name" => Some(&ctx.launcher_name),
        "launcher_version" => Some(&ctx.launcher_version),
        "classpath" => Some(&ctx.classpath),
        "library_directory" => Some(&ctx.library_directory),
        "resolution_width" => Some(&ctx.resolution_width),
        "resolution_height" => Some(&ctx.resolution_height),
        "quickPlaySingleplayer" => ctx.quick_play_singleplayer.as_deref(),
        "quickPlayMultiplayer" => ctx.quick_play_multiplayer.as_deref(),
        _ => None,
    }
}

pub fn build_args(
    version: &VersionInfo,
    ctx: &ArgContext,
    features: &FeatureSet,
) -> (Vec<String>, Vec<String>) {
    if let Some(args) = &version.arguments {
        (
            expand_args(&args.jvm, ctx, features),
            expand_args(&args.game, ctx, features),
        )
    } else if let Some(legacy) = &version.minecraft_arguments {
        let jvm = LEGACY_JVM.iter().map(|a| interpolate(a, ctx)).collect();
        let game = legacy
            .split_whitespace()
            .map(|token| interpolate(token, ctx))
            .collect();
        (jvm, game)
    } else {
        (Vec::new(), Vec::new())
    }
}

fn expand_args(args: &[LaunchArgument], ctx: &ArgContext, features: &FeatureSet) -> Vec<String> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            LaunchArgument::Value(value) => out.push(interpolate(value, ctx)),
            LaunchArgument::Ruled { rules, value } => {
                if rule_allows(rules, features) {
                    push_values(&mut out, value, ctx);
                }
            }
        }
    }
    out
}

fn push_values(out: &mut Vec<String>, value: &ArgValue, ctx: &ArgContext) {
    match value {
        ArgValue::One(s) => out.push(interpolate(s, ctx)),
        ArgValue::Many(many) => {
            for s in many {
                out.push(interpolate(s, ctx));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mojang::VersionInfo;

    fn load_fixture(name: &str) -> VersionInfo {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let text = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn sample_ctx() -> ArgContext {
        ArgContext {
            auth_player_name: "Steve".into(),
            auth_uuid: "00000000-0000-0000-0000-000000000000".into(),
            auth_access_token: "token".into(),
            user_type: "msa".into(),
            version_name: "1.21.1".into(),
            version_type: "release".into(),
            client_id: "client".into(),
            auth_xuid: String::new(),
            user_properties: "{}".into(),
            classpath_separator: ":".into(),
            game_directory: "/game".into(),
            assets_root: "/assets".into(),
            assets_index_name: "17".into(),
            natives_directory: "/natives".into(),
            launcher_name: "kmine".into(),
            launcher_version: "0.1.0".into(),
            classpath: "a.jar:b.jar".into(),
            library_directory: "/libs".into(),
            resolution_width: "854".into(),
            resolution_height: "480".into(),
            quick_play_singleplayer: None,
            quick_play_multiplayer: None,
        }
    }

    #[test]
    fn demo_arg_only_when_feature_set() {
        let v = load_fixture("version_1_21.json");
        let ctx = sample_ctx();
        let mut feat = FeatureSet::default();
        let (_, game) = build_args(&v, &ctx, &feat);
        assert!(!game.iter().any(|a| a == "--demo"));
        feat.demo = true;
        let (_, game) = build_args(&v, &ctx, &feat);
        assert!(game.iter().any(|a| a == "--demo"));
    }

    #[test]
    fn interpolates_player_name() {
        let v = load_fixture("version_1_12.json");
        let ctx = sample_ctx();
        let (_, game) = build_args(&v, &ctx, &FeatureSet::default());
        assert!(
            game.windows(2)
                .any(|w| w[0] == "--username" && w[1] == "Steve")
        );
    }

    #[test]
    fn interpolates_version_type() {
        let v = load_fixture("version_1_21.json");
        assert_eq!(v.version_type, "release");
        let ctx = sample_ctx();
        let (_, game) = build_args(&v, &ctx, &FeatureSet::default());
        assert!(
            game.windows(2)
                .any(|w| w[0] == "--versionType" && w[1] == "release"),
            "{game:?}"
        );
    }

    #[test]
    fn osx_jvm_flag_only_on_macos() {
        let v = load_fixture("version_1_21.json");
        let (jvm, _) = build_args(&v, &sample_ctx(), &FeatureSet::default());
        let has = jvm.iter().any(|a| a == "-XstartOnFirstThread");
        assert_eq!(has, cfg!(target_os = "macos"));
    }

    #[test]
    fn unknown_placeholder_stays() {
        let ctx = sample_ctx();
        assert_eq!(interpolate("x ${unknown} y", &ctx), "x ${unknown} y");
    }

    #[test]
    fn classpath_join_uses_platform_sep() {
        let paths = vec![
            std::path::PathBuf::from("a.jar"),
            std::path::PathBuf::from("b.jar"),
        ];
        let joined = join_classpath(&paths);
        if cfg!(windows) {
            assert_eq!(joined, "a.jar;b.jar");
        } else {
            assert_eq!(joined, "a.jar:b.jar");
        }
    }
}
