use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json;
use std::path::{Path, PathBuf};

use daedalus_core::layer::SerializableLayer;
use daedalus_core::registry::LayerRegistry;

#[derive(Args)]
pub struct RegistryArgs {
    #[command(subcommand)]
    pub command: RegistryCommand,

    /// Machine-readable plain output (no ANSI, no box drawing)
    #[arg(long, global = true)]
    pub plain: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

#[derive(Subcommand)]
pub enum RegistryCommand {
    /// Push a layer or full artifact to the registry
    Push(RegistryPushArgs),
    /// Pull a layer or artifact from the registry by hash
    Pull(RegistryPullArgs),
    /// List all layers in the local registry cache
    List(RegistryListArgs),
}

#[derive(Args)]
pub struct RegistryPushArgs {
    /// Path to the .daedalus file to push
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Layer name or hash to push from the artifact (push all layers if omitted)
    #[arg(long)]
    pub layer: Option<String>,

    /// Registry URL (use --local for a local directory cache instead)
    #[arg(long, env = "DAEDALUS_REGISTRY")]
    pub registry: Option<String>,

    /// Use a local directory as the registry cache instead of HTTP
    #[arg(long, value_name = "DIR")]
    pub local: Option<PathBuf>,

    /// Authentication token
    #[arg(long, env = "DAEDALUS_TOKEN")]
    pub token: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Machine-readable plain output (no ANSI, no box drawing)
    #[arg(long, global = true)]
    pub plain: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

#[derive(Args)]
pub struct RegistryPullArgs {
    /// Layer hash (or artifact manifest hash) to pull
    #[arg(value_name = "HASH")]
    pub hash: String,

    /// Output directory for the pulled layer (default: current dir)
    #[arg(short, long, default_value = ".")]
    pub output: PathBuf,

    /// Registry URL (use --local for a local directory cache instead)
    #[arg(long, env = "DAEDALUS_REGISTRY")]
    pub registry: Option<String>,

    /// Use a local directory as the registry cache instead of HTTP
    #[arg(long, value_name = "DIR")]
    pub local: Option<PathBuf>,

    /// Authentication token
    #[arg(long, env = "DAEDALUS_TOKEN")]
    pub token: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Machine-readable plain output (no ANSI, no box drawing)
    #[arg(long, global = true)]
    pub plain: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

#[derive(Args)]
pub struct RegistryListArgs {
    /// Local registry cache directory to list
    #[arg(long, default_value = "~/.daedalus/registry")]
    pub dir: PathBuf,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Machine-readable plain output (no ANSI, no box drawing)
    #[arg(long, global = true)]
    pub plain: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,
}

/// run - run.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
pub fn run(args: RegistryArgs) -> Result<()> {
    match args.command {
        RegistryCommand::Push(mut sub) => {
            sub.plain = args.plain;
            sub.no_input = args.no_input;
            run_push(sub)
        }
        RegistryCommand::Pull(mut sub) => {
            sub.plain = args.plain;
            sub.no_input = args.no_input;
            run_pull(sub)
        }
        RegistryCommand::List(mut sub) => {
            sub.plain = args.plain;
            sub.no_input = args.no_input;
            run_list(sub)
        }
    }
}

/// run_push - run push.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
fn run_push(mut args: RegistryPushArgs) -> Result<()> {
    let file = args.file.canonicalize().context("failed to find file")?;

    if file.extension().is_none_or(|e| e != "daedalus") {
        anyhow::bail!("{} is not a .daedalus file", file.display());
    }

    let (_footer, layers) = extract_layers_from_artifact(&file)?;

    if args.verbose {
        eprintln!("[daedalus] registry push: {}", file.display());
        eprintln!("  layers: {}", layers.len());
        for layer in &layers {
            eprintln!("  - {} (kind: {})", layer.name(), format_layer_kind(layer));
        }
    }

    match (&args.local, &args.registry) {
        (Some(local_dir), None) => {
            push_local(local_dir, &layers, &file, args.json)?;
        }
        (None, Some(registry_url)) => {
            if registry_url.contains("daedalus.example.com") {
                anyhow::bail!("cannot use placeholder registry URL '{registry_url}'");
            }
            push_remote(
                registry_url,
                &layers,
                &file,
                args.token.as_deref(),
                args.verbose,
                args.json,
            )?;
        }
        (None, None) => {
            anyhow::bail!("must specify --local <DIR> or --registry <URL>");
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("cannot use both --local and --registry");
        }
    }

    Ok(())
}

/// run_pull - run pull.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
fn run_pull(mut args: RegistryPullArgs) -> Result<()> {
    std::fs::create_dir_all(&args.output).context("failed to create output directory")?;

    match (&args.local, &args.registry) {
        (Some(local_dir), None) => {
            let mut reg = local_registry(local_dir)?;
            pull_from_store(&mut reg, &args.hash, &args.output, args.verbose, args.json)?;
        }
        (None, Some(registry_url)) => {
            if registry_url.contains("daedalus.example.com") {
                anyhow::bail!("cannot use placeholder registry URL '{registry_url}'");
            }
            pull_from_remote(
                registry_url,
                &args.hash,
                &args.output,
                args.token.as_deref(),
                args.verbose,
                args.json,
            )?;
        }
        (None, None) => {
            anyhow::bail!("must specify --local <DIR> or --registry <URL>");
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("cannot use both --local and --registry");
        }
    }

    Ok(())
}

/// run_list - run list.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
fn run_list(args: RegistryListArgs) -> Result<()> {
    let dir = expand_tilde(&args.dir);
    let reg = local_registry(&dir)?;

    let layers = reg.list_layers().unwrap_or_default();
    let count = layers.len();

    if args.json {
        let json_str = serde_json::to_string_pretty(&layers)?;
        if args.plain {
            println!("{json_str}");
        } else {
            println!("{json_str}");
        }
        return Ok(());
    }

    if args.verbose {
        eprintln!("[daedalus] registry list: {}", dir.display());
        eprintln!("  {count} layers");
    }

    if count == 0 {
        println!("(empty)");
        return Ok(());
    }

    if args.plain {
        for hash in &layers {
            println!("{hash}");
        }
    } else {
        let mut output = String::new();
        for hash in &layers {
            output.push_str(hash);
            output.push('\n');
        }
        crate::pager::page(&output)?;
    }

    Ok(())
}

/// local_registry - local registry.
/// @dir: directory path
///
/// Description:
///
/// Return: Result containing Result<LayerRegistry>
fn local_registry(dir: &Path) -> Result<LayerRegistry> {
    let path = expand_tilde(dir);
    std::fs::create_dir_all(&path).ok();
    LayerRegistry::disk(&path).context("failed to open local registry")
}

/// push_local - push local.
/// @dir: directory path
/// @layers: layers
/// @bin: bin
/// @json: json output
///
/// Description:
///
/// Return: Result containing Result<()>
fn push_local(dir: &Path, layers: &[SerializableLayer], bin: &Path, json: bool) -> Result<()> {
    let path = expand_tilde(dir);
    std::fs::create_dir_all(&path).context("failed to create local registry dir")?;
    let mut reg = LayerRegistry::disk(&path).context("failed to init local registry")?;

    for layer in layers {
        let hash = reg.push_layer(layer)?;
        if json {
            println!("{{\"pushed\":\"{}\",\"hash\":\"{hash}\"}}", layer.name());
        } else {
            println!("pushed layer '{}' -> {hash}", layer.name());
        }
    }

    let refs = build_layer_refs(&mut reg, layers)?;
    let manifest = daedalus_core::registry::LayerManifest {
        artifact_name: bin
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("artifact")
            .to_string(),
        layers: refs,
    };
    let manifest_hash = reg.publish_artifact(&manifest)?;
    if json {
        println!("{{\"published_artifact_manifest\":\"{manifest_hash}\"}}");
    } else {
        println!("published artifact manifest -> {manifest_hash}");
    }
    Ok(())
}

/// push_remote - push remote.
///
/// Description:
///
/// Return: nothing
fn push_remote(
    url: &str,
    _layers: &[SerializableLayer],
    bin: &Path,
    token: Option<&str>,
    verbose: bool,
    json: bool,
) -> Result<()> {
    let content = std::fs::read(bin).context("failed to read .daedalus file")?;
    let content_len = content.len();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    let mut request = client.post(url).body(content);

    if let Some(t) = token {
        request = request.bearer_auth(t);
    }

    if verbose {
        eprintln!("[daedalus] uploading {content_len} bytes to {url}");
    }

    let response = request.send().context("failed to upload to registry")?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "upload failed (HTTP {status}): {}",
            response.text().unwrap_or_default()
        );
    }

    if json {
        println!(
            "{{\"pushed\":\"{}\",\"url\":\"{}\",\"status\":{status}}}",
            bin.display(),
            url
        );
    } else {
        println!("pushed {} to {} (HTTP {status})", bin.display(), url);
    }
    Ok(())
}

/// Push a full `.daedalus` binary to a remote registry (used by `daedalus build --publish`).
pub fn push_remote_artifact(
    url: &str,
    bin: &Path,
    token: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let content = std::fs::read(bin).context("failed to read .daedalus file")?;
    let content_len = content.len();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    let mut request = client.post(url).body(content);
    if let Some(t) = token {
        request = request.bearer_auth(t);
    }

    if verbose {
        eprintln!("[daedalus] uploading {content_len} bytes to {url}");
    }

    let response = request.send().context("failed to upload to registry")?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "upload failed (HTTP {status}): {}",
            response.text().unwrap_or_default()
        );
    }

    println!("pushed {} to {} (HTTP {status})", bin.display(), url);
    Ok(())
}

/// pull_from_store - pull from store.
///
/// Description:
///
/// Return: nothing
fn pull_from_store(
    reg: &mut LayerRegistry,
    hash: &str,
    output: &Path,
    verbose: bool,
    json: bool,
) -> Result<()> {
    if verbose {
        eprintln!("[daedalus] pulling layer {hash} from local registry");
    }
    let layer = reg.pull_layer(hash)?;
    let out_file = output.join(format!("layer-{hash}.json"));
    std::fs::write(&out_file, serde_json::to_vec_pretty(&layer)?)
        .context("failed to write pulled layer")?;
    if json {
        println!(
            "{{\"pulled\":\"{hash}\",\"file\":\"{}\"}}",
            out_file.display()
        );
    } else {
        println!("pulled layer {hash} -> {}", out_file.display());
    }

    if let Ok(manifest) = reg.get_artifact(hash) {
        if json {
            println!("{{\"artifact_manifest\":\"{}\"}}", manifest.artifact_name);
        } else {
            println!(
                "(also retrieved artifact manifest: {})",
                manifest.artifact_name
            );
        }
    }

    Ok(())
}

/// pull_from_remote - pull from remote.
///
/// Description:
///
/// Return: nothing
fn pull_from_remote(
    url: &str,
    hash: &str,
    output: &Path,
    token: Option<&str>,
    verbose: bool,
    json: bool,
) -> Result<()> {
    let pull_url = format!("{url}/{hash}");
    if verbose {
        eprintln!("[daedalus] pulling {hash} from {pull_url}");
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    let mut request = client.get(&pull_url);
    if let Some(t) = token {
        request = request.bearer_auth(t);
    }

    let response = request.send().context("failed to pull from registry")?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "pull failed (HTTP {status}): {}",
            response.text().unwrap_or_default()
        );
    }

    let content = response.bytes().context("failed to read response body")?;
    let out_file = output.join(hash);
    std::fs::write(&out_file, &content).context("failed to write pulled content")?;
    if json {
        println!(
            "{{\"pulled\":\"{hash}\",\"file\":\"{}\",\"bytes\":{}}}",
            out_file.display(),
            content.len()
        );
    } else {
        println!(
            "pulled {hash} -> {} ({} bytes)",
            out_file.display(),
            content.len()
        );
    }
    Ok(())
}

/// build_layer_refs - build layer refs.
///
/// Description:
///
/// Return: nothing
fn build_layer_refs(
    reg: &mut LayerRegistry,
    layers: &[SerializableLayer],
) -> Result<Vec<daedalus_core::registry::LayerRef>> {
    let mut refs = vec![];
    for layer in layers {
        let hex = reg.push_layer(layer)?;
        let serialized =
            serde_json::to_vec(layer).map_err(|e| anyhow::anyhow!("serialize layer: {e}"))?;
        refs.push(daedalus_core::registry::LayerRef {
            hash: hex,
            name: layer.name().to_string(),
            kind: layer.kind(),
            size: serialized.len(),
        });
    }
    Ok(refs)
}

/// extract_layers_from_artifact - extract layers from artifact.
///
/// Description:
///
/// Return: nothing
pub fn extract_layers_from_artifact(
    bin: &Path,
) -> Result<(daedalus_core::format::Footer, Vec<SerializableLayer>)> {
    let mut file = std::fs::File::open(bin).context("failed to open .daedalus file")?;
    let footer = daedalus_core::format::Footer::read_from(&mut file)?;
    let meta_bytes = {
        use std::io::{Read, Seek, SeekFrom};
        let mut buf = vec![0u8; footer.meta_size as usize];
        file.seek(SeekFrom::Start(footer.meta_offset))?;
        file.read_exact(&mut buf)?;
        buf
    };
    let meta: serde_json::Value =
        serde_json::from_slice(&meta_bytes).context("failed to parse metadata JSON")?;
    let layers: Vec<SerializableLayer> =
        if let Some(arr) = meta.get("layers").and_then(|v| v.as_array()) {
            serde_json::from_value(serde_json::Value::Array(arr.clone()))
                .context("failed to parse layers from metadata")?
        } else {
            vec![]
        };
    Ok((footer, layers))
}

/// format_layer_kind - format layer kind.
/// @layer: layer
///
/// Description:
///
/// Return: the &'static str
fn format_layer_kind(layer: &SerializableLayer) -> &'static str {
    match layer.kind() {
        daedalus_core::layer::LayerKind::Runtime => "runtime",
        daedalus_core::layer::LayerKind::Config => "config",
        daedalus_core::layer::LayerKind::Custom => "custom",
    }
}

/// expand_tilde - expand tilde.
/// @path: file or directory path
///
/// Description:
///
/// Return: the PathBuf
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}
