// Runs the init scripts from src-tauri/src/windows.rs (COOKIE_SHIM, WS_SHIM) against
// minimal fakes of document/localStorage/__TAURI__ and checks their behaviour.
// Usage: node tools/test_shims.js
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert');

const src = fs.readFileSync(path.join(__dirname, '..', 'src-tauri', 'src', 'windows.rs'), 'utf8');
const script = (name) => {
  const m = src.match(new RegExp(`const ${name}: &str = r#"([\\s\\S]*?)"#;`));
  if (!m) throw new Error(`${name} not found`);
  return m[1];
};

function makeEnv() {
  const store = new Map();
  const calls = [];
  let channel = null;
  class Channel { constructor() { this.onmessage = null; channel = this; } }
  const ctx = {
    document: {},
    location: { href: 'tauri://localhost/index.html' },
    localStorage: {
      getItem: (k) => (store.has(k) ? store.get(k) : null),
      setItem: (k, v) => store.set(k, String(v)),
    },
    URL, EventTarget, Event, MessageEvent: class extends Event { constructor(t, i) { super(t); this.data = i.data; } },
    CloseEvent: class extends Event { constructor(t, i) { super(t); Object.assign(this, i); } },
    Blob, DOMException, ArrayBuffer, Uint8Array, setTimeout, console, JSON, Date, Number, String, Object, Array,
    WebSocket: class NativeWS { constructor(url) { this.native = true; this.url = url; } },
    __TAURI__: { core: { Channel, invoke: async (cmd, args) => { calls.push([cmd, args]); return cmd === 'ws_open' ? 7 : undefined; } } },
  };
  ctx.window = ctx;
  vm.createContext(ctx);
  return { ctx, store, calls, channel: () => channel };
}

(async () => {
  // ---- cookie shim
  {
    const { ctx } = makeEnv();
    vm.runInContext(script('COOKIE_SHIM'), ctx);
    const d = ctx.document;
    const exp = new Date(Date.now() + 86400e3).toUTCString();
    d.cookie = `kuro_infoToken=abc=123; expires=${exp}; path=/; SameSite=None; Secure`;
    d.cookie = 'kuro_lastLoginTime=1700000000; path=/';
    assert.strictEqual(d.cookie, 'kuro_infoToken=abc=123; kuro_lastLoginTime=1700000000');
    d.cookie = 'kuro_infoToken=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/';
    assert.strictEqual(d.cookie, 'kuro_lastLoginTime=1700000000');
    d.cookie = 'kuro_lastLoginTime=x; max-age=0';
    assert.strictEqual(d.cookie, '');
    // Frontend's own getCookie() parser
    d.cookie = 'kuro_infoToken=tok; path=/';
    const getCookie = (e) => { const t = `kuro_${e}=`; for (let o of d.cookie.split(';')) { while (o.charAt(0) === ' ') o = o.substring(1); if (o.indexOf(t) === 0) return o.substring(t.length); } return null; };
    assert.strictEqual(getCookie('infoToken'), 'tok');
    console.log('cookie shim: ok');
  }

  // ---- websocket shim
  {
    const { ctx, calls, channel } = makeEnv();
    vm.runInContext(script('WS_SHIM'), ctx);
    const other = new ctx.WebSocket('wss://example.com/x');
    assert.ok(other.native, 'non-Kuro host must use native WebSocket');

    const ws = new ctx.WebSocket('wss://api-mc-wuwatappo.kurogames-global.com/socket.io/?EIO=4&transport=websocket');
    assert.strictEqual(ws.readyState, 0);
    assert.strictEqual(ctx.WebSocket.OPEN, 1);
    assert.throws(() => ws.send('early'));
    const seen = [];
    ws.onopen = () => seen.push('open');
    ws.addEventListener('message', (e) => seen.push('msg:' + e.data));
    ws.onclose = (e) => seen.push(`close:${e.code}:${e.wasClean}`);
    await new Promise((r) => setTimeout(r, 0));
    assert.deepStrictEqual(calls[0][0], 'ws_open');
    assert.ok(calls[0][1].onEvent === channel());

    channel().onmessage({ type: 'open', protocol: '' });
    channel().onmessage({ type: 'text', data: '0{"sid":"x"}' });
    ws.send('40');
    ws.binaryType = 'arraybuffer';
    channel().onmessage({ type: 'binary', data: [1, 2, 3] });
    ws.send(new Uint8Array([9, 8]));
    ws.close(1000, 'bye');
    channel().onmessage({ type: 'close', code: 1000, reason: 'bye', wasClean: true });

    assert.deepStrictEqual(seen, ['open', 'msg:0{"sid":"x"}', 'msg:[object ArrayBuffer]', 'close:1000:true']);
    assert.deepStrictEqual(calls.slice(1).map((c) => c[0]), ['ws_send', 'ws_send', 'ws_close']);
    assert.strictEqual(JSON.stringify(calls[1][1]), JSON.stringify({ id: 7, text: '40' }));
    assert.strictEqual(JSON.stringify(calls[2][1]), JSON.stringify({ id: 7, binary: [9, 8] }));
    assert.strictEqual(JSON.stringify(calls[3][1]), JSON.stringify({ id: 7, code: 1000, reason: 'bye' }));
    assert.strictEqual(ws.readyState, 3);
    console.log('websocket shim: ok');
  }
})().catch((e) => { console.error(e); process.exit(1); });
