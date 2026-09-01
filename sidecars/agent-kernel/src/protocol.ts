export const AGENT_RPC_VERSION = 13;
export const AGENT_RPC_MAX_FRAME_BYTES = 1024 * 1024;
const LENGTH_PREFIX_BYTES = 4;

export interface AgentRpcEnvelope {
  protocolVersion: number;
  id: string;
  kind: string;
  payload: unknown;
}

export function encodeAgentRpcFrame(envelope: AgentRpcEnvelope): Buffer {
  const payload = Buffer.from(JSON.stringify(envelope), "utf8");
  if (payload.byteLength > AGENT_RPC_MAX_FRAME_BYTES) {
    throw new RangeError(
      `Agent RPC frame exceeds ${AGENT_RPC_MAX_FRAME_BYTES} bytes: ${payload.byteLength}`,
    );
  }

  const frame = Buffer.allocUnsafe(LENGTH_PREFIX_BYTES + payload.byteLength);
  frame.writeUInt32BE(payload.byteLength, 0);
  payload.copy(frame, LENGTH_PREFIX_BYTES);
  return frame;
}

export class AgentRpcFrameDecoder {
  private buffer = Buffer.alloc(0);

  push(chunk: Uint8Array): AgentRpcEnvelope[] {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const envelopes: AgentRpcEnvelope[] = [];
    let consumed = 0;

    while (this.buffer.byteLength - consumed >= LENGTH_PREFIX_BYTES) {
      const payloadLength = this.buffer.readUInt32BE(consumed);
      if (payloadLength > AGENT_RPC_MAX_FRAME_BYTES) {
        throw new RangeError(
          `Agent RPC frame exceeds ${AGENT_RPC_MAX_FRAME_BYTES} bytes: ${payloadLength}`,
        );
      }

      const frameLength = LENGTH_PREFIX_BYTES + payloadLength;
      if (this.buffer.byteLength - consumed < frameLength) {
        break;
      }

      const jsonStart = consumed + LENGTH_PREFIX_BYTES;
      const jsonEnd = jsonStart + payloadLength;
      const decoded: unknown = JSON.parse(this.buffer.toString("utf8", jsonStart, jsonEnd));
      envelopes.push(assertEnvelope(decoded));
      consumed += frameLength;
    }

    if (consumed > 0) {
      this.buffer = this.buffer.subarray(consumed);
    }

    return envelopes;
  }
}

function assertEnvelope(value: unknown): AgentRpcEnvelope {
  if (
    typeof value !== "object" ||
    value === null ||
    !("protocolVersion" in value) ||
    typeof value.protocolVersion !== "number" ||
    !("id" in value) ||
    typeof value.id !== "string" ||
    !("kind" in value) ||
    typeof value.kind !== "string" ||
    !("payload" in value)
  ) {
    throw new TypeError("Agent RPC frame does not match the v13 envelope");
  }

  return value as AgentRpcEnvelope;
}
