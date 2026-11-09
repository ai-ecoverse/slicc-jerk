const encoder = new TextEncoder();
const decoder = new TextDecoder();

class ProcessIO {
  constructor(ctx, request) {
    this.ctx = ctx;
    const env = { ...ctx.env };
    for (const name of request.remove_env) delete env[name];
    this.child = ctx.spawn({
      argv: request.argv,
      env,
      stdin: request.input === null ? (request.interactive ? 'inherit' : 'null') : 'pipe',
      stdout: 'pipe',
      stderr: 'pipe',
    });
    this.pending = new Map();
    this.output = { stdout: [], stderr: [] };
    this.input = request.input === null ? null : encoder.encode(request.input);
    this.written = 0;
    this.total = 0;
    this.status = 1;
  }
  submit(req, kind) {
    this.pending.set(this.ctx.async.submit(req), kind);
  }
  read(kind) {
    this.submit({ op: 'fd-read', fd: this.child[kind], max: 65536 }, kind);
  }
  write() {
    this.submit(
      { op: 'fd-write', fd: this.child.stdin, body: this.input.subarray(this.written) },
      'input'
    );
  }
  close(fd) {
    try {
      this.ctx.syscall({ op: 'fd-close', fd });
    } catch {}
  }
  complete(id) {
    const kind = this.pending.get(id);
    this.pending.delete(id);
    const result = this.ctx.async.take(id);
    if (result.error) throw new Error('kernel process I/O failed');
    if (kind === 'wait') {
      const raw = result.value[1];
      this.status = raw & 127 ? 128 + (raw & 127) : (raw >> 8) & 255;
    } else if (kind === 'input') {
      this.written += result.value;
      if (this.written < this.input.length) this.write();
      else this.close(this.child.stdin);
    } else if (result.value.length) {
      this.total += result.value.length;
      if (this.total > 8 * 1024 * 1024) throw new Error('provider output exceeds 8 MiB');
      this.output[kind].push(result.value);
      this.read(kind);
    }
  }
  run(timeout) {
    try {
      this.read('stdout');
      this.read('stderr');
      this.submit({ op: 'proc-wait', pid: this.child.pid, nohang: false }, 'wait');
      if (this.input !== null) this.write();
      const deadline = Date.now() + (timeout ?? 600000);
      while (this.pending.size) {
        const id = this.ctx.async.wait(Math.max(0, deadline - Date.now()));
        if (id === 0) return { timeout: true };
        this.complete(id);
      }
      return {
        code: this.status,
        stdout: text(this.output.stdout),
        stderr: text(this.output.stderr),
      };
    } finally {
      this.cleanup();
    }
  }
  cleanup() {
    this.input?.fill(0);
    for (const id of this.pending.keys()) this.ctx.async.cancel(id);
    if (this.pending.size) {
      try {
        this.ctx.kill(this.child.pid, 9);
        this.ctx.wait(this.child.pid);
      } catch {}
    }
    for (const kind of ['stdin', 'stdout', 'stderr']) {
      if (this.child[kind] !== undefined) this.close(this.child[kind]);
    }
    for (const parts of Object.values(this.output)) {
      for (const part of parts) part.fill(0);
    }
  }
}

function text(parts) {
  const bytes = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const part of parts) {
    bytes.set(part, at);
    at += part.length;
    part.fill(0);
  }
  const value = decoder.decode(bytes);
  bytes.fill(0);
  return value;
}

export function createImports(ctx) {
  let reply = new Uint8Array();
  return {
    jerk_process: {
      run(ptr, len) {
        try {
          const request = JSON.parse(decoder.decode(new Uint8Array(ctx.memory().buffer, ptr, len)));
          reply = encoder.encode(
            JSON.stringify(new ProcessIO(ctx, request).run(request.timeout_ms))
          );
        } catch {
          reply = encoder.encode(JSON.stringify({ error: 'cannot run provider in the kernel' }));
        }
        return reply.length;
      },
      take(ptr, len) {
        if (len !== reply.length) return 28;
        new Uint8Array(ctx.memory().buffer, ptr, len).set(reply);
        reply.fill(0);
        reply = new Uint8Array();
        return 0;
      },
    },
  };
}
