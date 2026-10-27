//! M12 release guard: version, bundle id, entitlements, crate manifests and workflow pinning stay consistent.

use std::path::Path;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const BUNDLE_ID: &str = "io.github.norberttak.hobbycam";

fn read(rel: &str) -> String {
    let p = Path::new(ROOT).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The `<string>`/`<true/>`… element that follows `<key>{key}</key>` in a plist.
fn plist_value<'a>(plist: &'a str, key: &str) -> &'a str {
    let tag = format!("<key>{key}</key>");
    let rest = &plist[plist
        .find(&tag)
        .unwrap_or_else(|| panic!("plist has no {key}"))
        + tag.len()..];
    let rest = rest.trim_start();
    let end = if rest.starts_with("<string>") {
        rest.find("</string>").expect("closed string") + "</string>".len()
    } else {
        rest.find('>').expect("element") + 1
    };
    &rest[..end]
}

/// `uses: owner/repo@<40 hex> # vX.Y.Z` (local `./` actions excepted).
fn pinned(line: &str) -> bool {
    let Some(spec) = line
        .trim_start()
        .trim_start_matches("- ")
        .strip_prefix("uses:")
    else {
        return true;
    };
    let spec = spec.trim();
    if spec.starts_with("./") {
        return true;
    }
    let Some((_, rest)) = spec.split_once('@') else {
        return false;
    };
    let (sha, comment) = rest.split_at(rest.len().min(40));
    let semver = |v: &str| {
        let parts: Vec<_> = v.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    };
    sha.len() == 40
        && sha.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
        && comment
            .strip_prefix(" # v")
            .is_some_and(|v| semver(v.trim_end()))
}

#[test]
fn changelog_has_current_version() {
    let head = format!("## [{}]", env!("CARGO_PKG_VERSION"));
    assert!(
        read("CHANGELOG.md").lines().any(|l| l.starts_with(&head)),
        "CHANGELOG.md has no `{head}` section"
    );
}

#[test]
fn info_plist_template() {
    let p = read("packaging/macos/Info.plist");
    let s = |v: &str| format!("<string>{v}</string>");
    assert_eq!(
        plist_value(&p, "CFBundleShortVersionString"),
        s("@VERSION@")
    );
    assert_eq!(plist_value(&p, "CFBundleVersion"), s("@BUILD@"));
    assert_eq!(
        plist_value(&p, "CFBundleExecutable"),
        s(env!("CARGO_PKG_NAME"))
    );
    assert_eq!(plist_value(&p, "CFBundleIdentifier"), s(BUNDLE_ID));
    assert_eq!(plist_value(&p, "LSMinimumSystemVersion"), s("11.0"));
    assert_eq!(plist_value(&p, "ITSAppUsesNonExemptEncryption"), "<false/>");
    assert!(read("scripts/macos-build.sh").contains("MACOSX_DEPLOYMENT_TARGET=11.0"));
}

#[test]
fn appstore_entitlements() {
    let e = read("packaging/macos/appstore.entitlements");
    assert_eq!(plist_value(&e, "com.apple.security.app-sandbox"), "<true/>");
    assert_eq!(
        plist_value(&e, "com.apple.security.files.user-selected.read-write"),
        "<true/>"
    );
    assert!(
        !e.contains("network"),
        "the App Store build needs no network"
    );
}

#[test]
fn crate_manifests() {
    let ws = read("Cargo.toml");
    assert!(ws.contains("members = [\"app\", \"crates/*\"]"));
    let mut manifests = vec!["app/Cargo.toml".to_string()];
    for e in std::fs::read_dir(Path::new(ROOT).join("crates")).unwrap() {
        let e = e.unwrap();
        if e.path().join("Cargo.toml").is_file() {
            manifests.push(format!(
                "crates/{}/Cargo.toml",
                e.file_name().to_string_lossy()
            ));
        }
    }
    assert_eq!(manifests.len(), 8, "app + 7 crates");
    for m in &manifests {
        let t = read(m);
        let lines: Vec<_> = t.lines().map(str::trim).collect();
        assert!(lines.contains(&"version.workspace = true"), "{m}: version");
        assert!(lines.contains(&"publish = false"), "{m}: publish");
        for l in lines.iter().filter(|l| l.contains("path =")) {
            assert!(
                !l.contains("version ="),
                "{m}: path dep with a version: {l}"
            );
        }
    }
}

#[test]
fn workflows_pinned() {
    let dir = Path::new(ROOT).join(".github/workflows");
    let mut all = String::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "yml") {
            all += &std::fs::read_to_string(&p).unwrap();
        }
    }
    let uses: Vec<_> = all.lines().filter(|l| l.contains("uses:")).collect();
    assert!(!uses.is_empty());
    for l in uses {
        assert!(pinned(l), "not pinned to a SHA with a version comment: {l}");
    }
    assert!(all.contains("mdbook@0.5.4") && all.contains("cargo-about@0.9.3"));

    let tc = read("rust-toolchain.toml");
    let channel = tc
        .lines()
        .find_map(|l| l.trim().strip_prefix("channel = \""))
        .and_then(|v| v.strip_suffix('"'))
        .expect("channel");
    let parts: Vec<_> = channel.split('.').collect();
    assert!(
        parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok()),
        "toolchain must be exact x.y.z, got {channel}"
    );
}

#[test]
fn bundle_id_agrees() {
    let ent = read("packaging/macos/appstore.entitlements");
    assert_eq!(
        plist_value(&ent, "com.apple.application-identifier"),
        format!("<string>@TEAM_ID@.{BUNDLE_ID}</string>")
    );
    assert!(read("packaging/macos/Info.plist").contains(BUNDLE_ID));
    assert!(read("docs/src/install.md").contains(&format!("~/Library/Containers/{BUNDLE_ID}/")));
}

#[test]
fn pinned_matcher() {
    let sha = "3d3c42e5aac5ba805825da76410c181273ba90b1";
    assert!(pinned(&format!(
        "      - uses: actions/checkout@{sha} # v7.0.1"
    )));
    assert!(pinned("    runs-on: ubuntu-latest"));
    assert!(!pinned("      - uses: actions/checkout@v4"));
    assert!(!pinned(&format!("      - uses: actions/checkout@{sha}")));
    assert!(!pinned(&format!("      - uses: a/b@{sha} # v7")));
    assert!(!pinned(&format!(
        "      - uses: a/b@{} # v1.2.3",
        sha.to_uppercase()
    )));
}

// ---- tester additions (M12 verification)

/// All `<key>` names of a plist, in order.
fn plist_keys(plist: &str) -> Vec<&str> {
    plist
        .split("<key>")
        .skip(1)
        .filter_map(|s| s.split_once("</key>").map(|(k, _)| k))
        .collect()
}

#[test]
fn appstore_entitlements_exact_keys() {
    assert_eq!(
        plist_keys(&read("packaging/macos/appstore.entitlements")),
        [
            "com.apple.security.app-sandbox",
            "com.apple.security.files.user-selected.read-write",
            "com.apple.application-identifier",
            "com.apple.developer.team-identifier",
        ],
        "no entitlement beyond sandbox + user-selected files (+ the two identifiers)"
    );
}

/// The `jobs.<name>:` block of a workflow (up to the next job at the same indent).
fn job<'a>(wf: &'a str, name: &str) -> &'a str {
    let start = wf
        .find(&format!("\n  {name}:\n"))
        .unwrap_or_else(|| panic!("no job {name}"))
        + 1;
    let rest = &wf[start..];
    let end = rest[1..]
        .match_indices("\n  ")
        .find(|(i, _)| {
            let c = rest.as_bytes()[1 + i + 3];
            c != b' ' && c != b'#'
        })
        .map_or(rest.len(), |(i, _)| i + 1);
    &rest[..end]
}

#[test]
fn release_workflow_guards() {
    let wf = read(".github/workflows/release.yml");
    assert!(
        !wf.contains("rust-cache"),
        "release builds must not use a cache"
    );
    assert!(wf.contains("--draft") && wf.contains("--verify-tag"));
    assert!(job(&wf, "windows").contains("VCRUNTIME140"));
    let mac = job(&wf, "macos");
    assert!(mac.contains("if: always()") && mac.contains("security delete-keychain"));
    assert!(mac.contains("::add-mask::"));
    assert!(
        !wf.contains("set -x"),
        "never trace a step that holds secrets"
    );
    let publish = job(&wf, "publish");
    assert!(publish.contains("contents: write"));
    assert!(
        wf.matches("contents: write").count() == 1,
        "write only in publish"
    );
    let ci = read(".github/workflows/ci.yml");
    let deploy = job(&ci, "deploy-docs");
    assert!(deploy.contains("github.event_name == 'push' && github.ref == 'refs/heads/main'"));
}

#[test]
fn release_publish_only_on_tag_push() {
    let wf = read(".github/workflows/release.yml");
    let publish = job(&wf, "publish");
    let cond = publish
        .lines()
        .find_map(|l| l.trim().strip_prefix("if:"))
        .expect("publish has an if:");
    assert!(
        cond.contains("github.event_name == 'push'"),
        "publish condition `{}` also matches a manual run on a tag",
        cond.trim()
    );
}
