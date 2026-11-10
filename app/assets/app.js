// The page side of aiterm-app: one xterm.js per tab, a tab bar, and keys a terminal wants
// rather than a browser. Bytes and keys go through the host (see app/src/app.rs).
'use strict';
const B = window.BOOT;
const send = (...a) => window.ipc.postMessage(a.join('\t'));
const bar = document.getElementById('tabs');
const plus = document.getElementById('plus');
const area = document.getElementById('terms');
const tabs = new Map();
let active = 0, fontSize = B.font, theme = B.theme;

const NAMES = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white'];

function mix(a, b, t) {
  const p = (h, i) => parseInt(h.slice(1 + 2 * i, 3 + 2 * i), 16);
  return '#' + [0, 1, 2].map(i => Math.round(p(a, i) * (1 - t) + p(b, i) * t).toString(16).padStart(2, '0')).join('');
}

function xtermTheme(t) {
  const x = {
    background: t.bg, foreground: t.fg, cursor: t.cursor, cursorAccent: t.bg,
    selectionBackground: mix(t.bg, t.fg, 0.3),
    scrollbarSliderBackground: t.fg + '22', scrollbarSliderHoverBackground: t.fg + '44',
    scrollbarSliderActiveBackground: t.fg + '66',
  };
  NAMES.forEach((n, i) => {
    x[n] = t.palette[i];
    x['bright' + n[0].toUpperCase() + n.slice(1)] = t.palette[i + 8];
  });
  return x;
}

function applyTheme(t) {
  theme = t;
  const s = document.documentElement.style;
  s.setProperty('--bg', t.bg);
  s.setProperty('--fg', t.fg);
  s.setProperty('--bar', mix(t.bg, '#000000', 0.14));
  s.setProperty('--muted', mix(t.fg, t.bg, 0.45));
  s.setProperty('--accent', t.palette[2]);
  for (const tab of tabs.values()) tab.term.options.theme = xtermTheme(t);
}

function show(id) {
  const tab = tabs.get(id);
  if (!tab) return;
  active = id;
  for (const t of tabs.values()) {
    t.el.classList.toggle('on', t === tab);
    t.btn.classList.toggle('on', t === tab);
  }
  tab.fit.fit();
  tab.term.focus();
  send('active', id);
}

function cycle(step) {
  const ids = [...tabs.keys()];
  if (ids.length < 2) return;
  show(ids[(ids.indexOf(active) + step + ids.length) % ids.length]);
}

function copy(term) {
  const text = term.getSelection();
  if (text) send('copy', text);
  term.clearSelection();
}

function zoom(step) {
  fontSize = step ? Math.min(48, Math.max(10, fontSize + step)) : 16;
  for (const t of tabs.values()) {
    t.term.options.fontSize = fontSize;
    t.fit.fit();
  }
  send('font', fontSize);
}

// What a terminal does with these keys; everything else goes to the shell as usual.
function action(e, tab) {
  if (!e.ctrlKey || e.altKey || e.metaKey) return null;
  const s = e.shiftKey, c = e.code;
  if (s && c === 'KeyT') return () => send('new', tab.id);
  if (s && c === 'KeyW') return () => send('close', tab.id);
  if (s && c === 'KeyN') return () => send('win', tab.id);
  if (c === 'Tab') return () => cycle(s ? -1 : 1);
  if (c === 'KeyC' && (s || tab.term.hasSelection())) return () => copy(tab.term);
  if (c === 'KeyV') return () => send('paste', tab.id);
  if (c === 'Equal' || c === 'NumpadAdd') return () => zoom(2);
  if (c === 'Minus' || c === 'NumpadSubtract') return () => zoom(-2);
  if (c === 'Digit0' || c === 'Numpad0') return () => zoom(0);
  return null;
}

function setTitle(tab, title) {
  // aiterm sets "aiterm · ~/some/folder": the tab shows the folder's name, the tooltip the path.
  const path = title.replace(/^aiterm\s*·\s*/, '');
  tab.label.textContent = path.split(/[\\/]/).filter(Boolean).pop() || path || 'aiterm';
  tab.btn.title = path;
}

function add(id) {
  const el = document.createElement('div');
  el.className = 'term';
  area.appendChild(el);
  const term = new Terminal({
    fontFamily: '"Cascadia Mono", Consolas, monospace',
    fontSize,
    theme: xtermTheme(theme),
    cursorBlink: true,
    scrollback: 10000,
    windowsPty: { backend: 'conpty', buildNumber: B.build },
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  const btn = document.createElement('div');
  btn.className = 'tab';
  btn.innerHTML = '<span>aiterm</span><b title="Close tab  (Ctrl+Shift+W)">×</b>';
  bar.insertBefore(btn, plus);
  const tab = { id, el, term, fit, btn, label: btn.firstChild };
  tabs.set(id, tab);

  term.open(el);
  // WebGL draws block and box characters (aiterm's ▀▄█ wordmark) as whole cells and is faster;
  // the DOM renderer stays as the fallback.
  try {
    const gl = new WebglAddon.WebglAddon();
    gl.onContextLoss(() => gl.dispose());
    term.loadAddon(gl);
  } catch { }
  term.onData(d => send('i', id, d));
  term.onResize(({ cols, rows }) => send('r', id, cols, rows));
  term.onTitleChange(t => setTitle(tab, t));
  term.onBell(() => send('bell'));
  term.attachCustomKeyEventHandler(e => {
    const act = action(e, tab);
    if (!act) return true;
    if (e.type === 'keydown') {
      e.preventDefault();
      act();
    }
    return false;
  });
  // Right-click copies a selection, otherwise pastes (like the Windows console).
  el.addEventListener('contextmenu', e => {
    e.preventDefault();
    if (term.hasSelection()) copy(term);
    else send('paste', id);
  });
  btn.addEventListener('mousedown', e => {
    if (e.button === 1 || e.target.tagName === 'B') send('close', id);
    else if (e.button === 0) show(id);
    e.preventDefault();
  });

  show(id);
  send('spawn', id, term.cols, term.rows);
  return tab;
}

window.T = {
  add,
  out(id, b64) {
    const tab = tabs.get(id);
    if (!tab) return;
    const s = atob(b64), u = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) u[i] = s.charCodeAt(i);
    // A Uint8Array lets xterm.js finish UTF-8 sequences split across chunks itself.
    tab.term.write(u, () => send('a', id));
  },
  remove(id) {
    const tab = tabs.get(id);
    if (!tab) return;
    const ids = [...tabs.keys()], i = ids.indexOf(id);
    tabs.delete(id);
    tab.term.dispose();
    tab.el.remove();
    tab.btn.remove();
    if (active === id && tabs.size) show(ids[i + 1] ?? ids[i - 1]);
  },
  paste(id, text) {
    tabs.get(id)?.term.paste(text);
  },
  theme: applyTheme,
};

plus.addEventListener('mousedown', e => {
  e.preventDefault();
  send('new', active);
});
document.addEventListener('wheel', e => { if (e.ctrlKey) e.preventDefault(); }, { passive: false });
let pendingFit = 0;
new ResizeObserver(() => {
  cancelAnimationFrame(pendingFit);
  pendingFit = requestAnimationFrame(() => tabs.forEach(t => t.fit.fit()));
}).observe(area);

applyTheme(theme);
add(B.first);
// Two frames: the first one with the terminal in it has been painted, so the window can appear.
requestAnimationFrame(() => requestAnimationFrame(() => send('ready')));
