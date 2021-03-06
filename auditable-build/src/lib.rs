#![forbid(unsafe_code)]

//! This is a companion crate to [`auditable`](https://docs.rs/auditable/) to be used as a build dependency.
//!
//! This crate is responsible for collecting the dependecy data. It exists as a separate crate purely for technical reasons.
//! Please refer to [`auditable`](https://docs.rs/auditable/) crate for documentation.

use std::{env, path::{Path, PathBuf}, fs::File, io::Write};
use std::{convert::TryFrom, collections::HashSet};
use auditable_serde::VersionInfo;
use miniz_oxide::deflate::compress_to_vec_zlib;
use cargo_metadata::{Metadata, MetadataCommand};

/// Run this in your build.rs to collect dependency info and make it avaible to `inject_dependency_list!` macro
pub fn collect_dependency_list() {
    let version_info = VersionInfo::try_from(&get_metadata()).unwrap();
    let json = serde_json::to_string(&version_info).unwrap();
    let compressed_json = compress_to_vec_zlib(json.as_bytes(), choose_compression_level());
    let output_file_path = output_file_path();
    write_dependency_info(&compressed_json, &output_file_path);
    export_dependency_file_path(&output_file_path);
}

fn output_file_path() -> std::path::PathBuf {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_dir = Path::new(&out_dir);
    dest_dir.join("dependency-list.json.zlib")
}

fn write_dependency_info(data: &[u8], path: &Path) {
    let f = File::create(path).unwrap();
    let mut writer = std::io::BufWriter::new(f);
    writer.write_all(data).unwrap();
}

fn export_dependency_file_path(path: &Path) {
    // Required because there's no cross-platform way to use `include_bytes!`
    // on a file from the build dir other than this. I've tried lots of them.
    // See https://github.com/rust-lang/rust/issues/75075
    println!("cargo:rustc-env=RUST_AUDIT_DEPENDENCY_FILE_LOCATION={}", path.display());
}

fn choose_compression_level() -> u8 {
    let build_profile = env::var("PROFILE").unwrap();
    match build_profile.as_str() {
        "debug" => 1,
        "release" => 7, // not 9 because this also affects speed of incremental builds
        _ => panic!("Unknown build profile: {}", &build_profile)
    }
}

fn get_metadata() -> Metadata {
    let mut metadata_command = metadata_command();
    let mut features = enabled_features();
    // feature "default" is explicitly passed to build scripts but there is no "all" feature
    if let Some(index) = features.iter().position(|x| x.as_str() == "default") {
        features.remove(index);
    } else {
        metadata_command.features(cargo_metadata::CargoOpt::NoDefaultFeatures);
    }
    metadata_command.features(cargo_metadata::CargoOpt::SomeFeatures(features));
    metadata_command.exec().unwrap()
}

fn metadata_command() -> MetadataCommand {
    // MetadataCommand::new() automatically reads the $CARGO env var
    // that Cargo sets for build scripts, so we don't have to pass it explicitly
    let mut cmd = MetadataCommand::new();
    let cargo_toml_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml");
    cmd.manifest_path(cargo_toml_path);
    cmd.other_options(vec!["--filter-platform=".to_owned() + &env::var("TARGET").unwrap()]);
    cmd
}

fn enabled_features() -> Vec<String> {
    let mut result = Vec::new();
    // Cargo irreparably mangles the feature list when passing it to the build script
    // (in particular, case and distinction between `-` and `_` are lost, see
    // https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts)
    // so we have to reconsruct it by calling cargo-metadata and filtering features
    // that we know exist against the mangled list of *enabled* features from env variables
    let enabled_uppercase_features = enabled_uppercase_features();
    let dry_run_metadata = metadata_command().exec().unwrap();
    // we can safely unwrap here because resolve is only missing if called with --no-deps,
    // and root package is only missing in a virtual workspace, from which you can't run a build script
    let root_package_id = dry_run_metadata.resolve.unwrap().root.unwrap();
    let root_package = dry_run_metadata.packages.iter().filter(|p| p.id == root_package_id).next().unwrap();
    for (feature, _implied_features) in root_package.features.iter() {
        let mangled_feature = feature.to_ascii_uppercase().replace("-", "_");
        if enabled_uppercase_features.contains(&mangled_feature) {
            result.push(feature.clone());
        }
    }
    result
}

fn enabled_uppercase_features() -> HashSet<String> {
    let mut features = HashSet::new();
    for (var_name, _value) in env::vars().filter(|(name, _value)| {
        name.len() > "CARGO_FEATURE_".len() && name.starts_with("CARGO_FEATURE_")
    }) {
        features.insert(var_name.trim_start_matches("CARGO_FEATURE_").to_owned());
    }
    features
}
