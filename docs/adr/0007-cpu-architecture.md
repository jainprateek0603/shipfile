# ADR-0007: Default to arm64 (Graviton) for Fargate and build images for it

**Status:** accepted · November 2026

## Context
Docker on an Apple Silicon Mac builds `arm64` images by default, while a Fargate task definition runs `X86_64` unless told otherwise. Shipping an arm64 image to an x86 task fails at start with "exec format error", which is confusing and only shows up after a deploy.

The options were:
- (a) always build `linux/amd64`: works everywhere, but builds on ARM laptops run under emulation and are much slower
- (b) run tasks on arm64 (Graviton) and build `linux/arm64`: about 20% cheaper on Fargate, native on ARM laptops, emulated only on x86 machines and CI

## Decision
Option (b), made explicit:
- A spec field `aws.arch: arm64 | x86_64`, default `arm64`.
- The TaskDefinition sets `RuntimePlatform.CpuArchitecture` from it (`ARM64` or `X86_64`), and `OperatingSystemFamily: LINUX`.
- Every image is built with `docker buildx build --platform linux/<arch>`, locally and in `apply`, so local and AWS run the same image.
- The generated CI workflow sets up QEMU and buildx when the runner's architecture differs (GitHub also offers native `ubuntu-24.04-arm` runners).
- `doctor` checks that `docker buildx` is available.

## Consequences
- No "exec format error" surprise: the architecture is always stated, never inferred.
- Base images in the generated Dockerfiles must be multi-arch (the official Node, Python, Go and Rust images are).
- Teams with x86-only native dependencies set `arch: x86_64` and accept slower builds on ARM laptops.
- Changing `arch` replaces the task definition but not data-holding resources, so the safety guard is not involved.
