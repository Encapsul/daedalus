"""xbin inspect: display .xbin contents without extracting."""

from __future__ import annotations

import hashlib
import json
import struct

from . import format as fmt


def inspect(path: str) -> None:
    footer = fmt.read_footer(path)
    with open(path, "rb") as f:
        f.seek(footer.meta_offset)
        meta = json.loads(f.read(footer.meta_size))

    arch = fmt.ARCH_NAMES.get(footer.arch, f"0x{footer.arch:02x}")
    signed = "yes" if footer.flags & fmt.FLAG_SIGNED else "no"

    print(f"file:            {path}")
    print(f"format version:  {footer.format_version}")
    print(f"architecture:    {arch}")
    print(f"signed:          {signed}")
    if signed and footer.sig_offset:
        with open(path, "rb") as f:
            sig_data = fmt.read_at(f, footer.sig_offset, 68)
        sig_size = struct.unpack_from("<I", sig_data, 0)[0]
        sig = sig_data[4:68]
        fp = hashlib.sha256(sig[:32]).hexdigest()
        print(f"sig offset:      {footer.sig_offset}")
        print(f"sig size:        {sig_size} bytes")
        print(f"signer fp:       {fp}")
    print(f"name:            {meta.get('name')}")
    print(f"runtime:         {meta.get('runtime')}")
    print(f"isolation level: {meta.get('isolation')}")
    print(f"entrypoint:      {' '.join(meta.get('entrypoint', []))}")
    print(f"cwd:             {meta.get('cwd')}")
    if meta.get("env"):
        print("env:")
        for k, v in meta["env"].items():
            print(f"  {k}={v}")
    print(f"created:         {meta.get('created')}")

    layers = meta.get("layers")
    if layers:
        print("layers:")
        for layer in layers:
            print(f"  - {layer['kind']:<8} {layer['csize']/1e6:5.1f}MB compressed "
                  f"/ {layer['usize']/1e6:5.1f}MB raw  sha256:{layer['sha256'][:12]}…")
    else:
        print(f"payload:         {footer.payload_csize/1e6:.1f}MB compressed "
              f"/ {footer.payload_usize/1e6:.1f}MB raw")
    print(f"integrity sha256:{footer.payload_sha256.hex()}")
