import {
  FrameDecoder,
  SerializedFrameWriter,
} from '../../src/script-host/framing';
import type {
  ExecuteMessage,
  ReadyMessage,
  ScriptHostProtocolVersion,
  SuccessMessage,
} from '../../src/script-host/types';

async function writeStdout(frame: Uint8Array): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    process.stdout.write(frame, error => {
      if (error === null || error === undefined) resolve();
      else reject(error);
    });
  });
}

function resultFor(nodeId: string): SuccessMessage['outcome']['value'] {
  switch (nodeId) {
    case 'prepare':
      return { pullRequest: 42 };
    case 'createNetwork':
      return { networkId: 'network-42' };
    case 'provision:compensate:createNetwork':
      return { deleted: 'network-42' };
    case 'provision:cleanup:revokeCredentials':
      return { revoked: true };
    default:
      return { completed: true };
  }
}

const decoder = new FrameDecoder();
const writer = new SerializedFrameWriter(writeStdout);
const protocolVersion = Number(
  process.env.WOML_SCRIPT_HOST_PROTOCOL_VERSION ?? '11'
) as ScriptHostProtocolVersion;
const ready: ReadyMessage = {
  protocol: 'woml.script-host',
  protocolVersion,
  messageType: 'ready',
  hostInstanceId: 'host_nonresponsive_cancel',
};

await writer.send(ready);
const reader = Bun.stdin.stream().getReader();
while (true) {
  const { done, value } = await reader.read();
  if (done) break;
  for (const candidate of decoder.push(value)) {
    const message = candidate as {
      readonly messageType?: string;
      readonly nodeId?: string;
    };
    // Model the Windows failure deterministically: deployment never completes,
    // and its cancellation request is deliberately ignored. The Rust runtime
    // must retire this host before running compensation on a fresh process.
    if (message.messageType === 'cancel') continue;
    if (message.messageType !== 'execute') continue;
    const request = candidate as ExecuteMessage;
    if (request.nodeId === 'deployApplication') continue;
    const completed: SuccessMessage = {
      protocol: 'woml.script-host',
      protocolVersion: request.protocolVersion,
      messageType: 'completed',
      invocationId: request.invocationId,
      outcome: { kind: 'success', value: resultFor(request.nodeId) },
      durationMs: 0,
    };
    await writer.send(completed);
  }
}
decoder.finish();
await writer.drain();
