"""xbin inspect: display .xbin contents without extracting."""

from __future__ import annotations

import json
import struct

from . import format as fmt


def _collect_inspect_data(path: str) -> dict:
    footer = fmt.read_footer(path)
    with open(path, "rb") as f:
        f.seek(footer.meta_offset)
        meta = json.loads(f.read(footer.meta_size))

    arch = fmt.ARCH_NAMES.get(footer.arch, f"0x{footer.arch:02x}")
    signed = bool(footer.flags & fmt.FLAG_SIGNED)

    data: dict = {
        "file": path,
        "format_version": footer.format_version,
        "architecture": arch,
        "signed": signed,
    }
    if signed:
        with open(path, "rb") as f:
            sig_data = fmt.read_at(f, footer.sig_offset, fmt.SIG_BLOCK_SIZE)
        sig_size = struct.unpack_from("<I", sig_data, 0)[0]
        data["sig_offset"] = footer.sig_offset
        data["sig_size"] = sig_size
    data["name"] = meta.get("name")
    data["runtime"] = meta.get("runtime")
    data["isolation"] = meta.get("isolation")
    data["entrypoint"] = meta.get("entrypoint", [])
    data["env"] = meta.get("env", {})
    data["created"] = meta.get("created")
    data["layers"] = meta.get("layers")
    data["integrity_sha256"] = footer.payload_sha256.hex()
    data["version"] = meta.get("version", "")
    data["author"] = meta.get("author", "")
    data["description"] = meta.get("description", "")
    data["license"] = meta.get("license", "")
    return data


def inspect(path: str, *, json_output: bool = False) -> None:
    data = _collect_inspect_data(path)

    if json_output:
        print(json.dumps(data, indent=2))
        return

    signed = data["signed"]
    print(f"file:            {data['file']}")
    print(f"format version:  {data['format_version']}")
    print(f"architecture:    {data['architecture']}")
    print(f"signed:          {'yes' if signed else 'no'}")
    if signed:
        print(f"sig offset:      {data['sig_offset']}")
        print(f"sig size:        {data['sig_size']} bytes")
        print("signer:          (run 'xbin verify' with trusted keys to identify)")
    print(f"name:            {data['name']}")
    if data["version"]:
        print(f"version:         {data['version']}")
    if data["author"]:
        print(f"author:          {data['author']}")
    if data["description"]:
        print(f"description:     {data['description']}")
    if data["license"]:
        print(f"license:         {data['license']}")
    print(f"runtime:         {data['runtime']}")
    print(f"isolation level: {data['isolation']}")
    print(f"entrypoint:      {' '.join(data['entrypoint'])}")
    if data["env"]:
        print("env:")
        for k, v in data["env"].items():
            print(f"  {k}={v}")
    print(f"created:         {data['created']}")

    layers = data["layers"]
    if layers:
        print("layers:")
        for layer in layers:
            print(
                f"  - {layer['kind']:<8} {layer['csize']/1e6:5.1f}MB compressed "
                f"/ {layer['usize']/1e6:5.1f}MB raw  sha256:{layer['sha256'][:12]}\u2026"
            )
    else:
        print("payload:         N/A")
    print(f"integrity sha256:{data['integrity_sha256']}")
