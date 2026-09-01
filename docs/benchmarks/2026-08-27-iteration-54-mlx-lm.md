# Iteration 54 — MLX-LM Apple Silicon real acceptance

- Date: 2026-08-27
- HAL100: 1.0.4 development baseline
- Cell: `mlx-lm / official-http-server / macOS aarch64 / Metal / local loopback`
- Result: PASS (1/1 explicit real acceptance)

## Environment

- Host: Apple M1, 16 GiB, macOS, aarch64, Metal
- Service: official `mlx_lm.server`, bound to `127.0.0.1:18080`
- Runtime: `mlx-lm==0.31.3`, `mlx==0.32.2`
- Model: `mlx-community/Qwen3-0.6B-4bit`

## Evidence

1. `/health` returned the official `{"status":"ok"}` shape.
2. `/v1/models` returned a unique model catalog entry with `object=model`.
3. Unary Chat Completions produced exactly one `hal100_protocol_probe` tool call and positive prompt/
   completion usage.
4. Streaming Chat Completions produced choices, a non-null finish reason, positive usage with
   `include_usage`, and `[DONE]`.
5. The official `system_fingerprint` was stable across unary and stream responses and yielded
   MLX-LM `0.31.3`.
6. HAL100 persisted the explicit `engine=mlx-lm`, `adapterVariant=official-http-server` binding,
   saved a spec-v3 runtime profile, activated it, rechecked it after route switch, and verified the
   active profile.

## Negative evidence

`mlx-community/Qwen2.5-0.5B-Instruct-4bit` served successfully but did not produce the required tool
call; HAL100 returned `QualificationFailed`. This is an intentional fail-closed model capability
result, not a port-health success.

## Scope

This evidence promotes only the Apple Silicon/Metal loopback cell. It does not claim Intel Mac,
Windows, Linux, remote deployment, larger-context capacity, or other MLX model templates.
