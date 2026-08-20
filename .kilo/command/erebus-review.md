# /erebus-review — Security audit of erebus build

Loads gstack's `/cso` (Chief Security Officer) skill to run an OWASP + STRIDE
audit on the erebus stub, unsafe code, Ed25519 key handling, and SISR update
pipeline.

## Steps
1. Run `/cso` in the gstack skill
2. Focus on: stub/src/main.rs unsafe blocks, exec.rs mount operations,
   format.rs magic constants, SISR engine in erebus-core/src/sisr/
3. Check for: memory safety, privilege escalation, token/key leakage
