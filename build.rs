use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;

const CAVA_VERSION: &str = "0.10.6";

fn main() {
    if let Err(e) = real_main() {
        // Make it obvious why the build failed.
        eprintln!("bundle-cava failed: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_BUNDLE_CAVA");
    println!("cargo:rerun-if-env-changed=CLI_MUSIC_PLAYER_CAVA_BUNDLE_VERSION");
    println!("cargo:rerun-if-env-changed=CLI_MUSIC_PLAYER_CAVA_BUNDLE_URL");
    println!("cargo:rerun-if-env-changed=CLI_MUSIC_PLAYER_CAVA_BUNDLE_SKIP");

    // Only run when the feature is enabled.
    if std::env::var_os("CARGO_FEATURE_BUNDLE_CAVA").is_none() {
        return Ok(());
    }

    if std::env::var_os("CLI_MUSIC_PLAYER_CAVA_BUNDLE_SKIP").is_some() {
        println!("cargo:warning=bundle-cava: skipped (CLI_MUSIC_PLAYER_CAVA_BUNDLE_SKIP is set)");
        return Ok(());
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").context("OUT_DIR missing")?);

    // Where we place the final cava binary so runtime can find it via `current_exe().parent()/cava`.
    let bin_dir = find_target_profile_dir(&out_dir)
        .with_context(|| format!("cannot locate target profile dir from OUT_DIR={}", out_dir.display()))?;
    let target_cava = bin_dir.join("cava");

    if target_cava.is_file() {
        // Assume already built for this profile.
        println!(
            "cargo:warning=bundle-cava: using existing {}",
            target_cava.display()
        );
        ensure_out_dir_copy(&target_cava, &out_dir)?;
        return Ok(());
    }

    let version = std::env::var("CLI_MUSIC_PLAYER_CAVA_BUNDLE_VERSION").unwrap_or_else(|_| CAVA_VERSION.to_string());
    let url = std::env::var("CLI_MUSIC_PLAYER_CAVA_BUNDLE_URL").unwrap_or_else(|_| {
        format!("https://github.com/karlstav/cava/archive/refs/tags/{version}.tar.gz")
    });

    let work_dir = out_dir.join("bundle-cava");
    let src_dir = work_dir.join("src");
    let tarball = work_dir.join("cava.tar.gz");

    fs::create_dir_all(&work_dir).context("create work dir")?;

    // Download.
    if !tarball.is_file() {
        println!("cargo:warning=bundle-cava: downloading {url}");
        download_to(&url, &tarball).with_context(|| format!("download {url}"))?;
    }

    // Extract.
    if !src_dir.is_dir() {
        println!("cargo:warning=bundle-cava: extracting");
        extract_tar_gz(&tarball, &work_dir).context("extract tarball")?;

        // The archive extracts into `cava-<tag>` (and tags may be prefixed like `CAVA-0.10.6`).
        let extracted = find_first_dir_named_prefix(&work_dir, "cava-")
            .or_else(|| find_first_dir_named_prefix(&work_dir, "CAVA-"))
            .ok_or_else(|| anyhow!("cannot find extracted cava-* directory in {}", work_dir.display()))?;

        // Normalize path.
        fs::rename(&extracted, &src_dir).or_else(|_| {
            // If rename fails (e.g., cross-device), fall back to copy.
            copy_dir_recursive(&extracted, &src_dir)
        })?;
    }

    // Build (autotools).
    // NOTE: This requires system tools + deps: autoconf/automake/libtool/pkgconf and fftw/iniparser,
    // plus at least one audio backend dev package (pipewire/pulse/alsa/etc) for capturing.
    println!("cargo:warning=bundle-cava: building cava from source");

    // Workaround: some distros ship an `AX_CHECK_GL` macro that can make cava's autotools bootstrap
    // fail during `aclocal`/`autoconf`. We inject a minimal override macro via `ACLOCAL_PATH` so that
    // `autogen.sh` succeeds even when OpenGL checks are not available.
    // This only affects the optional SDL/GLSL output path and keeps core/raw output working.
    let aclocal_override_dir = work_dir.join("aclocal-override");
    ensure_ax_check_gl_override(&aclocal_override_dir)
        .context("prepare aclocal override")?;

    // Run autogen only if configure hasn't been generated yet.
    if !src_dir.join("configure").is_file() {
        run_in_env(
            &src_dir,
            "sh",
            &["-c", "chmod +x ./autogen.sh && ./autogen.sh"],
            "autogen",
            &[(
                "ACLOCAL_PATH",
                prepend_env_path(&aclocal_override_dir, "ACLOCAL_PATH"),
            )],
        )?;
    }

    // Keep configure default (auto-detect). Users can override by setting CLI_MUSIC_PLAYER_CAVA_BUNDLE_URL
    // to a fork or a patched tarball if needed.
    run_in(&src_dir, "sh", &["-c", "./configure"], "configure")?;

    // Use -j when available.
    let jobs = std::env::var("NUM_JOBS").ok();
    let make_cmd = match jobs {
        Some(j) if !j.trim().is_empty() => format!("make -j{j}"),
        _ => "make".to_string(),
    };
    run_in(&src_dir, "sh", &["-c", &make_cmd], "make")?;

    // Copy artifact.
    let built = src_dir.join("cava");
    if !built.is_file() {
        return Err(anyhow!("expected built cava at {}", built.display()));
    }

    fs::create_dir_all(&bin_dir).ok();
    fs::copy(&built, &target_cava).with_context(|| {
        format!(
            "copy built cava from {} to {}",
            built.display(),
            target_cava.display()
        )
    })?;

    // Ensure it's executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target_cava)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target_cava, perms)?;
    }

    ensure_out_dir_copy(&target_cava, &out_dir)?;

    println!(
        "cargo:warning=bundle-cava: installed {}",
        target_cava.display()
    );

    Ok(())
}

fn download_to(url: &str, dst: &Path) -> Result<()> {
    let resp = ureq::get(url)
        .set("User-Agent", "cli-music-player build.rs")
        .call()
        .with_context(|| format!("GET {url}"))?;

    if resp.status() >= 400 {
        return Err(anyhow!("HTTP {} for {url}", resp.status()));
    }

    let mut reader = resp.into_reader();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).context("read response")?;
    fs::write(dst, buf).with_context(|| format!("write {}", dst.display()))?;
    Ok(())
}

fn extract_tar_gz(tar_gz: &Path, dest_dir: &Path) -> Result<()> {
    let f = fs::File::open(tar_gz).with_context(|| format!("open {}", tar_gz.display()))?;
    let gz = GzDecoder::new(f);
    let mut ar = Archive::new(gz);
    ar.unpack(dest_dir)
        .with_context(|| format!("unpack into {}", dest_dir.display()))?;
    Ok(())
}

fn find_first_dir_named_prefix(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if name.starts_with(prefix) {
                return Some(p);
            }
        }
    }
    None
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("mkdir {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read_dir {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if meta.is_file() {
            fs::copy(&from, &to).with_context(|| format!("copy {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn run_in(dir: &Path, program: &str, args: &[&str], label: &str) -> Result<()> {
    let status = Command::new(program)
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| format!("run {label}: {program} {:?}", args))?;
    if !status.success() {
        return Err(anyhow!("{label} failed with {status}"));
    }
    Ok(())
}

fn run_in_env(
    dir: &Path,
    program: &str,
    args: &[&str],
    label: &str,
    envs: &[(&str, String)],
) -> Result<()> {
    let mut cmd = Command::new(program);
    cmd.current_dir(dir).args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let status = cmd
        .status()
        .with_context(|| format!("run {label}: {program} {:?}", args))?;
    if !status.success() {
        return Err(anyhow!("{label} failed with {status}"));
    }
    Ok(())
}

fn ensure_ax_check_gl_override(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("mkdir {}", dir.display()))?;
    let p = dir.join("ax_check_gl.m4");
    if p.is_file() {
        return Ok(());
    }

    // Minimal stub: always take ACTION-IF-NOT-FOUND (2nd arg) when provided.
    // This avoids failing the bootstrap while keeping cava build functional for raw output.
    let content = r#"dnl build.rs override: provide a minimal AX_CHECK_GL so cava's autogen doesn't fail
AC_DEFUN([AX_CHECK_GL],[
  m4_ifval([$2],[$2],[:])
])
"#;
    fs::write(&p, content).with_context(|| format!("write {}", p.display()))?;
    Ok(())
}

fn prepend_env_path(dir: &Path, var: &str) -> String {
    let head = dir.display().to_string();
    match std::env::var(var) {
        Ok(prev) if !prev.trim().is_empty() => format!("{head}:{prev}"),
        _ => head,
    }
}

fn find_target_profile_dir(out_dir: &Path) -> Option<PathBuf> {
    // Typical OUT_DIR:
    //   target/debug/build/<crate>-<hash>/out
    // or:
    //   target/<triple>/release/build/<crate>-<hash>/out
    // We want the directory that contains the binary: target/(<triple>/)?<profile>

    let mut cur = out_dir;
    while let Some(parent) = cur.parent() {
        if cur.file_name().and_then(|s| s.to_str()) == Some("build") {
            // parent is <profile>
            return Some(parent.to_path_buf());
        }
        cur = parent;
    }
    None
}

fn ensure_out_dir_copy(built: &Path, out_dir: &Path) -> Result<()> {
    let dst = out_dir.join("cava.bin");
    if dst.is_file() {
        return Ok(());
    }
    fs::copy(built, &dst).with_context(|| {
        format!(
            "copy built cava from {} to {}",
            built.display(),
            dst.display()
        )
    })?;
    Ok(())
}
