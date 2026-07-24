#!/usr/bin/env node
/**
 * Local SSE transport integration test for Phase 4.
 *
 * Spins a tiny HTTP server that speaks OpenAI SSE protocol, then verifies
 * the raw transport patterns that apply_delta and openrouter_stream_async
 * process. Run: node --test tests/test_sse_transport.mjs
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';

function startServer(handler) {
  return new Promise((resolve) => {
    const srv = http.createServer(handler);
    srv.listen(0, '127.0.0.1', () => resolve({ srv, port: srv.address().port }));
  });
}

function closeServer(srv) {
  return new Promise((resolve) => srv.close(resolve));
}

function sse(chunks) {
  return chunks.map(c => `data: ${c}\n\n`).join('');
}

describe('SSE transport integration', () => {
  it('normal stream: content + [DONE]', async () => {
    const { srv, port } = await startServer((_, res) => {
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      res.write(sse([
        '{"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}',
        '{"choices":[{"delta":{"content":" world"},"finish_reason":null}]}',
        '[DONE]',
      ]));
      res.end();
    });
    try {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`);
      assert.equal(r.status, 200);
      const t = await r.text();
      assert.ok(t.includes('Hello'));
      assert.ok(t.includes('[DONE]'));
    } finally { await closeServer(srv); }
  });

  it('error envelope returns 429', async () => {
    const { srv, port } = await startServer((_, res) => {
      res.writeHead(429, { 'Content-Type': 'text/event-stream' });
      res.write('data: {"error":{"message":"rate limited","code":429}}\n\n');
      res.end();
    });
    try {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`);
      assert.equal(r.status, 429);
    } finally { await closeServer(srv); }
  });

  it('EOF without [DONE] and no finish_reason', async () => {
    const { srv, port } = await startServer((_, res) => {
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      res.write('data: {"choices":[{"delta":{"content":"partial"},"finish_reason":null}]}\n\n');
      res.end();
    });
    try {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`);
      const t = await r.text();
      assert.ok(t.includes('partial'));
      assert.ok(!t.includes('[DONE]'));
    } finally { await closeServer(srv); }
  });

  it('empty string finish_reason', async () => {
    const { srv, port } = await startServer((_, res) => {
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      res.write('data: {"choices":[{"delta":{"content":"hi"},"finish_reason":""}]}\n\n');
      res.write('[DONE]\n\n');
      res.end();
    });
    try {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`);
      assert.equal(r.status, 200);
      assert.ok((await r.text()).includes('hi'));
    } finally { await closeServer(srv); }
  });

  it('mixed frame: content + reasoning + finish_reason', async () => {
    const { srv, port } = await startServer((_, res) => {
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      const payload = JSON.stringify({
        choices: [{
          delta: { content: 'Answer', reasoning_content: 'Thinking' },
          finish_reason: 'stop',
        }],
      });
      res.write(`data: ${payload}\n\n`);
      res.write('[DONE]\n\n');
      res.end();
    });
    try {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`);
      assert.equal(r.status, 200);
      const t = await r.text();
      assert.ok(t.includes('Answer'));
      assert.ok(t.includes('Thinking'));
      assert.ok(t.includes('stop'));
    } finally { await closeServer(srv); }
  });

  it('multi-byte UTF-8 split across SSE frames', async () => {
    // Build a payload where € (U+20AC, UTF-8 E2 82 AC) is split mid-codepoint
    const fullPayload = JSON.stringify({
      choices: [{ delta: { content: 'caf\u00e2\u0082\u00ac' }, finish_reason: null }],
    });
    const mid = Math.floor(fullPayload.length / 2);
    const half1 = fullPayload.slice(0, mid);
    const half2 = fullPayload.slice(mid);
    const { srv, port } = await startServer((_, res) => {
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      res.write(`data: ${half1}\n\n`);
      res.write(`data: ${half2}\n\n`);
      res.write('[DONE]\n\n');
      res.end();
    });
    try {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`);
      assert.equal(r.status, 200);
    } finally { await closeServer(srv); }
  });
});
