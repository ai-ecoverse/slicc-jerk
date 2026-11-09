import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';
import { createNodeKernel } from '@ai-ecoverse/slicc-kernel/node';

test('browser-machine loopback and kernel loopback are different listeners', async () => {
  const host = createServer((_, res) => res.end('browser-machine'));
  await new Promise((resolve) => host.listen(0, '127.0.0.1', resolve));
  const port = host.address().port;
  const kernel = await createNodeKernel({
    cdp: async () => {
      throw new Error('unused stub');
    },
  });
  let ready;
  const listening = new Promise((resolve) => {
    ready = resolve;
  });
  const pkg = '/node_modules/loopback-probe';
  await kernel.writeFile(
    `${pkg}/package.json`,
    JSON.stringify({
      name: 'loopback-probe',
      slicc: { abi: 'js', commands: { probe: { module: 'probe.mjs' } } },
    })
  );
  await kernel.writeFile(
    `${pkg}/probe.mjs`,
    `export default async function(ctx) {
    if (ctx.argv[1] === 'client') {
      const response = await ctx.fetch('http://127.0.0.1:${port}/callback');
      await ctx.write(1, await response.text());
      return 0;
    }
    const listener = await ctx.net.listen({ port: ${port} });
    await ctx.write(1, JSON.stringify({ cdp: ctx.env.SLICC_CDP_URL, proxy: ctx.env.https_proxy }));
    const connection = await listener.accept();
    const writer = connection.writable.getWriter();
    await writer.write(new TextEncoder().encode('HTTP/1.1 200 OK\\r\\nContent-Length: 6\\r\\nConnection: close\\r\\n\\r\\nkernel'));
    await writer.close();
    await connection.close();
    await listener.close();
    return 0;
  }`
  );
  try {
    const server = kernel.run(['probe'], { onStdout: ready });
    const env = JSON.parse(await listening);
    assert.match(env.cdp, /^ws:\/\/127\.0\.0\.1:/);
    assert.match(env.proxy, /^http:\/\/127\.0\.0\.1:/);
    const external = await fetch(`http://127.0.0.1:${port}/callback`);
    assert.equal(await external.text(), 'browser-machine');
    const internal = await kernel.run(['probe', 'client']);
    assert.equal(internal.status, 0, internal.stderr);
    assert.equal(internal.stdout, 'kernel');
    assert.equal((await server).status, 0);
    console.log(
      `measured kernel CDP=${env.cdp}, HTTPS proxy=${env.proxy}; same-port loopback listeners are independent`
    );
  } finally {
    kernel.terminate();
    await new Promise((resolve) => host.close(resolve));
  }
});
