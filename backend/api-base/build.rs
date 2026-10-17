// Fetch the disposable-email domain blocklist at build time.
//
// The list is deliberately NOT stored in the repository. This build script
// downloads it from a pinned upstream commit into OUT_DIR, where
// `include_str!` in src/application/http/auth/util.rs picks it up. Pinning
// the commit and verifying the sha256 keeps every build byte-identical
// despite the network fetch: a moved/rewritten upstream ref can never change
// what lands in the binary, and bumping the pin below is the reviewed diff
// that ships a list update.
//
// Source:  https://github.com/disposable-email-domains/disposable-email-domains
// License: CC0 1.0 (public domain, no attribution required)
//
// To bump the pinned list:
//   1. curl -sSL -o /tmp/list.conf \
//      https://raw.githubusercontent.com/disposable-email-domains/disposable-email-domains/<NEW_SHA>/disposable_email_blocklist.conf
//   2. sha256sum /tmp/list.conf
//   3. update UPSTREAM_COMMIT_SHA and EXPECTED_SHA256 below
//
// Caching & offline builds: the fetched copy lives in OUT_DIR and is reused
// as long as it still matches EXPECTED_SHA256, so only the first build (or a
// cargo clean) needs network access. Set HERALD_DISPOSABLE_LIST_URL to fetch
// from a mirror instead of raw.githubusercontent.com; the content is still
// hash-verified, so a mirror can serve the pinned bytes but nothing else.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};

const UPSTREAM_COMMIT_SHA: &str = "aa6cbc8922833cbf384e3204fba01bb2ad6d8177";
const EXPECTED_SHA256: &str = "5052d5eebd1b8907738dbbf3f1f4cd961593c14f44435674bc6d8a34986009a7";

fn main() {
    // Source edits must not trigger network I/O; refetch only when the pin
    // itself (this file) or the mirror override changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=HERALD_DISPOSABLE_LIST_URL");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let dest = out_dir.join("disposable_email_blocklist.conf");

    if let Ok(cached) = fs::read(&dest)
        && hex::encode(Sha256::digest(&cached)) == EXPECTED_SHA256
    {
        return; // pinned copy already fetched by an earlier build
    }

    // An exported-but-empty override (CI rendering an unset variable, an
    // empty build-arg) must fall back to the pinned URL, not poison the
    // fetch: ureq on an empty URL can only fail, and the panic message would
    // point at the very variable causing it.
    let url = std::env::var("HERALD_DISPOSABLE_LIST_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "https://raw.githubusercontent.com/disposable-email-domains/disposable-email-domains/{UPSTREAM_COMMIT_SHA}/disposable_email_blocklist.conf"
            )
        });

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let body = agent
        .get(&url)
        .call()
        .unwrap_or_else(|error| {
            panic!(
                "failed to download the disposable-email blocklist from {url}: {error}; \
                 set HERALD_DISPOSABLE_LIST_URL to a reachable mirror or check the network"
            )
        })
        .into_string()
        .unwrap_or_else(|error| {
            panic!("failed to read the disposable-email blocklist response: {error}")
        });

    let actual = hex::encode(Sha256::digest(body.as_bytes()));
    assert_eq!(
        actual, EXPECTED_SHA256,
        "downloaded disposable-email blocklist does not match the pinned content \
         (expected sha256 {EXPECTED_SHA256}, got {actual})"
    );

    fs::write(&dest, body).expect("failed to write the blocklist into OUT_DIR");
}
