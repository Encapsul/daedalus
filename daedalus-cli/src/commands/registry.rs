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
#[allow(clippy::struct_excessive_bools)]
pub struct RegistryPushArgs {
    /// Path to the .de/.daedalus file to push
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Publish the full runnable binary under this name[:tag] instead of layers
    #[arg(long, value_name = "NAME[:TAG]")]
    pub name: Option<String>,

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
#[allow(clippy::struct_excessive_bools)]
pub struct RegistryPullArgs {
    /// Layer hash (or artifact manifest hash) to pull
    #[arg(value_name = "HASH", required_unless_present = "name")]
    pub hash: Option<String>,

    /// Pull the runnable binary by NAME[:TAG] instead of by hash
    #[arg(long, value_name = "NAME[:TAG]")]
    pub name: Option<String>,

    /// Output directory (by hash) or output file path (by name)
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
#[allow(clippy::struct_excessive_bools)]
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

/// run - dispatch a registry subcommand (push, pull, or list).
/// @args: command arguments
///
/// Description:
/// Routes to the appropriate subcommand handler based on the RegistryCommand enum.
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

/// run_push - push a .de file to a registry, as layers or as a runnable binary.
/// @args: command arguments
///
/// Description:
/// `--name NAME[:TAG]` publishes the full runnable binary (so another machine
/// can pull and run it); otherwise each layer is pushed content-addressed and
/// an artifact manifest published.
///
/// Return: Result containing Result<()>
fn run_push(args: RegistryPushArgs) -> Result<()> {
    let file = args.file.canonicalize().context("failed to find file")?;

    let ext = file.extension().and_then(|e| e.to_str());
    if !matches!(ext, Some("de" | "daedalus")) {
        anyhow::bail!("{} is not a .de/.daedalus file", file.display());
    }

    if let Some(name_tag) = &args.name {
        let (name, tag) = split_name_tag(name_tag)?;
        return push_binary(
            &file,
            &name,
            &tag,
            args.local.as_ref(),
            args.registry.as_deref(),
            args.token.as_deref(),
            args.verbose,
        );
    }

    let (_footer, layers) = extract_layers_from_artifact(&file)?;
    let chosen: Vec<&SerializableLayer> = match &args.layer {
        Some(wanted) => {
            let picked: Vec<&SerializableLayer> =
                layers.iter().filter(|l| l.name() == wanted).collect();
            if picked.is_empty() {
                anyhow::bail!("layer '{}' not found in artifact", wanted);
            }
            picked
        }
        None => layers.iter().collect(),
    };

    if args.verbose {
        eprintln!("[daedalus] registry push: {}", file.display());
        eprintln!("  layers: {}", chosen.len());
        for layer in &chosen {
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
            push_layers_remote(registry_url, &layers, args.token.as_deref(), args.verbose)?;
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

/// push_binary - publish a full runnable `.de` binary under `name:tag`.
/// @file: the .de file
/// @name: artifact name
/// @tag: artifact tag
/// @local / @registry: exactly one target
/// @token: optional bearer token
/// @verbose: verbose output
///
/// Description:
/// Local targets store the bytes content-addressed; remote targets POST them
/// to the `/artifact` endpoint.
///
/// Return: Result containing Result<()>
fn push_binary(
    file: &Path,
    name: &str,
    tag: &str,
    local: Option<&PathBuf>,
    registry: Option<&str>,
    token: Option<&str>,
    verbose: bool,
) -> Result<()> {
    match (local, registry) {
        (Some(local_dir), None) => {
            let mut reg = local_registry(local_dir)?;
            let bytes = std::fs::read(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let hash = reg
                .publish_artifact_binary(name, tag, &bytes)
                .context("failed to publish binary")?;
            println!("published '{name}:{tag}' -> {hash} ({} bytes)", bytes.len());
            Ok(())
        }
        (None, Some(registry_url)) => {
            if registry_url.contains("daedalus.example.com") {
                anyhow::bail!("cannot use placeholder registry URL '{registry_url}'");
            }
            push_binary_remote(registry_url, name, tag, file, token, verbose)
        }
        (None, None) => anyhow::bail!("must specify --local <DIR> or --registry <URL>"),
        (Some(_), Some(_)) => anyhow::bail!("cannot use both --local and --registry"),
    }
}

/// split_name_tag - parse and validate "name[:tag]" with a `latest` default.
/// @spec: name[:tag]
///
/// Description:
/// Delegates to the shared core validator so push, pull, and the serve GET
/// route all agree on exactly which aliases are fetchable.
///
/// Return: Result containing a tuple of (name, tag)
fn split_name_tag(spec: &str) -> Result<(String, String)> {
    let (name, tag) = daedalus_core::http_parse::split_name_tag(spec)?;
    Ok((name.to_string(), tag.to_string()))
}

/// push_binary_remote - POST a runnable binary to the remote `/artifact` endpoint.
/// @base: registry base URL
/// @name: name
/// @tag: tag
/// @bin: file to upload
/// @token: optional bearer token
/// @verbose: verbose output
///
/// Description:
/// Appends `/artifact?name=..&tag=..` to the base URL if not already present.
///
/// Return: Result containing Result<()>
fn push_binary_remote(
    base: &str,
    name: &str,
    tag: &str,
    bin: &Path,
    token: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let content = std::fs::read(bin).context("failed to read file")?;
    let content_len = content.len();

    let url = format!("{}?name={name}&tag={tag}", endpoint_url(base, "/artifact"));

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    let mut request = client.post(url).body(content);
    if let Some(t) = token {
        request = request.bearer_auth(t);
    }

    if verbose {
        eprintln!("[daedalus] publishing '{name}:{tag}' ({content_len} bytes) to {base}");
    }

    let response = request.send().context("failed to publish to registry")?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "publish failed (HTTP {status}): {}",
            response.text().unwrap_or_default()
        );
    }
    let hash = response.text().unwrap_or_default();
    println!("published '{name}:{tag}' -> {hash} ({content_len} bytes)");
    Ok(())
}

/// push_layers_remote - push each layer as JSON to the remote `/push` endpoint.
/// @base: registry base URL
/// @layers: layers to push
/// @token: optional bearer token
/// @verbose: verbose output
///
/// Description:
/// Sends each serialized layer to `<base>/push` and prints its content hash.
///
/// Return: Result containing Result<()>
fn push_layers_remote(
    base: &str,
    layers: &[SerializableLayer],
    token: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let url = endpoint_url(base, "/push");
    for layer in layers {
        let body = serde_json::to_vec(layer).context("serialize layer")?;
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_mins(5))
            .build()
            .context("failed to create HTTP client")?;
        let mut request = client.post(&url).body(body);
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        let response = request.send().context("failed to push layer")?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!(
                "layer push failed (HTTP {status}): {}",
                response.text().unwrap_or_default()
            );
        }
        let hash = response.text().unwrap_or_default();
        if verbose {
            eprintln!("[daedalus] pushed layer '{}' -> {hash}", layer.name());
        }
        println!("pushed layer '{}' -> {hash}", layer.name());
    }
    Ok(())
}

/// endpoint_url - append a default endpoint to a registry base URL, avoiding
/// duplication when the base already ends with it.
/// @base: registry base URL (may already end with `default_endpoint`)
/// @default_endpoint: e.g. "/artifact"
///
/// Return: the joined URL with exactly one separator
fn endpoint_url(base: &str, default_endpoint: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with(default_endpoint) {
        base.to_string()
    } else {
        format!("{base}{default_endpoint}")
    }
}

/// run_pull - pull a layer by hash or a runnable binary by name from a registry.
/// @args: command arguments
///
/// Description:
/// `--name NAME[:TAG]` fetches the runnable binary and writes it to the output
/// file path (default `./NAME.de`); a positional hash pulls a layer JSON into
/// the output directory.
///
/// Return: Result containing Result<()>
fn run_pull(args: RegistryPullArgs) -> Result<()> {
    if let Some(name_tag) = &args.name {
        let (name, tag) = split_name_tag(name_tag)?;
        let out_file: PathBuf = if args.output.is_dir() {
            args.output.join(format!("{name}.de"))
        } else {
            args.output
        };
        let out_ref = &out_file;

        match (&args.local, &args.registry) {
            (Some(local_dir), None) => {
                let reg = local_registry(local_dir)?;
                let bytes = reg
                    .artifact_binary(&name, &tag)?
                    .ok_or_else(|| anyhow::anyhow!("artifact '{name}:{tag}' not found"))?;
                write_pulled_binary(&bytes, out_ref, &name, &tag, args.verbose)?;
            }
            (None, Some(registry_url)) => {
                if registry_url.contains("daedalus.example.com") {
                    anyhow::bail!("cannot use placeholder registry URL '{registry_url}'");
                }
                pull_binary_remote(
                    registry_url,
                    &name,
                    &tag,
                    out_ref,
                    args.token.as_deref(),
                    args.verbose,
                )?;
            }
            (None, None) => {
                anyhow::bail!("must specify --local <DIR> or --registry <URL>");
            }
            (Some(_), Some(_)) => {
                anyhow::bail!("cannot use both --local and --registry");
            }
        }
        return Ok(());
    }

    let hash = args
        .hash
        .as_deref()
        .expect("registry pull requires a hash or --name");

    std::fs::create_dir_all(&args.output).context("failed to create output directory")?;

    match (&args.local, &args.registry) {
        (Some(local_dir), None) => {
            let mut reg = local_registry(local_dir)?;
            pull_from_store(&mut reg, hash, &args.output, args.verbose, args.json)?;
        }
        (None, Some(registry_url)) => {
            if registry_url.contains("daedalus.example.com") {
                anyhow::bail!("cannot use placeholder registry URL '{registry_url}'");
            }
            pull_from_remote(
                registry_url,
                hash,
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

/// write_pulled_binary - write fetched binary bytes to an output file.
/// @bytes: the binary
/// @out_file: destination path
/// @name: artifact name (for display)
/// @tag: artifact tag (for display)
/// @verbose: verbose output
///
/// Description:
/// Creates parent dirs and writes atomically via a temp file + rename.
///
/// Return: Result containing Result<()>
fn write_pulled_binary(
    bytes: &[u8],
    out_file: &Path,
    name: &str,
    tag: &str,
    verbose: bool,
) -> Result<()> {
    if let Some(parent) = out_file.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).context("failed to create output dir")?;
        }
    }
    let tmp = out_file.with_extension("part");
    std::fs::write(&tmp, bytes).context("failed to write pulled binary")?;
    std::fs::rename(&tmp, out_file).context("failed to finalize pulled binary")?;
    if verbose {
        eprintln!(
            "[daedalus] pulled '{name}:{tag}' -> {} ({} bytes)",
            out_file.display(),
            bytes.len()
        );
    } else {
        println!(
            "pulled '{name}:{tag}' -> {} ({} bytes)",
            out_file.display(),
            bytes.len()
        );
    }
    Ok(())
}

/// pull_binary_remote - GET a runnable binary by name from the remote `/artifact` endpoint.
/// @base: registry base URL
/// @name: name
/// @tag: tag
/// @out_file: destination path
/// @token: optional bearer token
/// @verbose: verbose output
///
/// Description:
/// Downloads and writes the binary, returning its length.
///
/// Return: Result containing Result<()>
fn pull_binary_remote(
    base: &str,
    name: &str,
    tag: &str,
    out_file: &Path,
    token: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let base = base.trim_end_matches('/');
    let base = base.strip_suffix("/artifact").unwrap_or(base);
    let url = format!("{base}/artifact/{name}:{tag}");
    if verbose {
        eprintln!("[daedalus] pulling '{name}:{tag}' from {url}");
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .build()
        .context("failed to create HTTP client")?;

    let mut request = client.get(&url);
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
    write_pulled_binary(&content, out_file, name, tag, verbose)
}

/// run_list - list all layers in the local registry cache.
/// @args: command arguments
///
/// Description:
/// Reads the local registry directory and prints all stored layer hashes.
///
/// Return: Result containing Result<()>
fn run_list(args: RegistryListArgs) -> Result<()> {
    let dir = expand_tilde(&args.dir);
    let reg = local_registry(&dir)?;

    let layers = reg.list_layers().unwrap_or_default();
    let count = layers.len();

    if args.json {
        let json_str = serde_json::to_string_pretty(&layers)?;
        println!("{json_str}");
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

/// local_registry - open or create a local directory-backed registry.
/// @dir: directory path
///
/// Description:
/// Creates the directory if needed and opens a LayerRegistry on disk.
///
/// Return: Result containing Result<LayerRegistry>
fn local_registry(dir: &Path) -> Result<LayerRegistry> {
    let path = expand_tilde(dir);
    std::fs::create_dir_all(&path).ok();
    LayerRegistry::disk(&path).context("failed to open local registry")
}

/// push_local - push layers to a local directory registry.
/// @dir: directory path
/// @layers: layers
/// @bin: bin
/// @json: json output
///
/// Description:
/// Stores each layer in the local registry directory and publishes an artifact
/// manifest referencing them.
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

/// Push a full `.de` binary to a remote registry as `<stem>:latest`.
/// Used by `daedalus build --publish <URL>`.
pub fn push_remote_artifact(
    url: &str,
    bin: &Path,
    token: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let name = bin
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("artifact")
        .to_string();
    push_binary_remote(url, &name, "latest", bin, token, verbose)
}

/// pull_from_store - pull a layer from a local registry by hash.
/// @reg: layer registry
/// @hash: layer hash
/// @output: output directory
/// @verbose: verbose output
/// @json: json output
///
/// Description:
/// Reads the layer JSON from the local store and writes it to the output dir.
/// Also retrieves the artifact manifest if the hash matches one.
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

/// pull_from_remote - pull a layer from a remote HTTP registry by hash.
/// @url: registry base URL
/// @hash: layer hash
/// @output: output directory
/// @token: optional bearer token
/// @verbose: verbose output
/// @json: json output
///
/// Description:
/// GETs `url/hash` and writes the response bytes to `output/hash`.
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

/// build_layer_refs - push each layer and collect LayerRef entries.
/// @reg: layer registry
/// @layers: layers to register
///
/// Description:
/// Serializes each layer to JSON and records its hash in the registry.
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

/// extract_layers_from_artifact - read footer and metadata from a .daedalus file.
/// @bin: path to the .daedalus binary
///
/// Description:
/// Parses the footer and metadata JSON to extract the layers array.
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

/// format_layer_kind - convert a LayerKind to its string representation.
/// @layer: layer
///
/// Description:
/// Maps Runtime→"runtime", Config→"config", Custom→"custom".
///
/// Return: the &'static str
fn format_layer_kind(layer: &SerializableLayer) -> &'static str {
    match layer.kind() {
        daedalus_core::layer::LayerKind::Runtime => "runtime",
        daedalus_core::layer::LayerKind::Config => "config",
        daedalus_core::layer::LayerKind::Custom => "custom",
    }
}

/// expand_tilde - expand a leading ~ to the user's home directory.
/// @path: file or directory path
///
/// Description:
/// Replaces a leading "~/" with $HOME if set; otherwise returns the path unchanged.
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
