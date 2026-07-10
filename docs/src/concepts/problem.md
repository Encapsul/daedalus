# The Problem

Distributing a server application is unnecessarily complicated.

An app depends on a set of things installed on the developer's machine:

- a **runtime** (Node.js, Python, Java, Ruby...);
- **system shared libraries** (`.so` files);
- **packages** (`node_modules`, pip packages, gems...);
- **configuration files** and assets.

When you give this app to someone else — a colleague, a production server,
a client — **it breaks**. Node is not installed, or it's the wrong version,
or a system library is missing, or the paths are different.

This is the classic *"it works on my machine"* problem.

## Why Docker is not enough (for this case)

Docker solves part of the problem but introduces significant friction:

- you must **install Docker** (root daemon, system service);
- you must **understand** images, registries, volumes, networking;
- the daemon runs **constantly**, often as root;
- it's **heavy** for simply distributing a tool or a small service.

Docker remains excellent for orchestration, multi-container, Kubernetes.
But for *"here, run this server"*, it's overkill.

## The xbin idea

A single self-contained binary file that contains absolutely everything the
app needs, and runs like a normal program.

```bash
chmod +x my_app.xbin && ./my_app.xbin
```

Zero installation. Zero configuration. The file is self-sufficient.
