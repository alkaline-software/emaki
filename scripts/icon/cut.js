// The app icon, cut out of the logo JP had generated (logo.png beside this
// script): a terracotta plate on a white ground, carrying a cream scroll
// with a dark terminal panel on its open sheet. The script finds the plate,
// makes everything outside it transparent, un-blends the anti-aliased edge
// from the white it was drawn on, and writes every size the platforms want.
//
//   node cut.js <out-dir>    writes icon-1024.png, the smaller PNGs, icon.ico
//                            and an icon.iconset/ (full-bleed) for iconutil.
const { createCanvas, loadImage, ImageData } = require('@napi-rs/canvas');
const fs = require('fs'), path = require('path');

const S = 1024;
// The plate's side on the 1024 canvas. 824 is the rounded square on Apple's
// icon grid, so the Dock shows this icon at the size it shows every other.
const PLATE = 824;
const WHITE = 254;         // the ground's value in every channel
const EDGE = 200;          // how far the plate's orange sits from white

async function cutout() {
  const im = await loadImage(path.join(__dirname, 'logo.png'));
  const W = im.width, H = im.height;
  const c = createCanvas(W, H), x = c.getContext('2d');
  x.drawImage(im, 0, 0);
  const img = x.getImageData(0, 0, W, H), d = img.data;
  const dev = (i) => Math.max(WHITE - d[i], WHITE - d[i + 1], WHITE - d[i + 2], 0);

  // The plate is whatever is not white, plus the bounding box around it.
  const inside = new Uint8Array(W * H);
  let l = W, r = 0, t = H, b = 0;
  for (let y = 0; y < H; y++) for (let xx = 0; xx < W; xx++) {
    const p = y * W + xx;
    if (dev(p * 4) > 8) { inside[p] = 1; if (xx < l) l = xx; if (xx > r) r = xx; if (y < t) t = y; if (y > b) b = y; }
  }
  // Pixels that touch the outside are the plate's anti-aliased edge: their
  // coverage is how far they sit from white, and their colour is what is
  // left once that much white is taken back out.
  const edge = (p) => {
    const y = (p / W) | 0, xx = p % W;
    for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) {
      const yy = y + dy, xs = xx + dx;
      if (yy < 0 || yy >= H || xs < 0 || xs >= W || !inside[yy * W + xs]) return true;
    }
    return false;
  };
  for (let p = 0; p < W * H; p++) {
    const i = p * 4;
    if (!inside[p]) { d[i + 3] = 0; continue; }
    if (!edge(p)) continue;
    const a = Math.min(1, dev(i) / EDGE);
    for (let k = 0; k < 3; k++) d[i + k] = Math.max(0, Math.min(255, Math.round((d[i + k] - (1 - a) * WHITE) / a)));
    d[i + 3] = Math.round(a * 255);
  }
  x.putImageData(img, 0, 0);
  return { canvas: c, box: [l, t, r - l + 1, b - t + 1] };
}

// Area-averaging resample of a premultiplied RGBA buffer, so a 16px icon is
// the mean of its pixels rather than a sample of a few of them.
function shrink(src, sw, sh, dw, dh) {
  const out = new Uint8ClampedArray(dw * dh * 4);
  for (let y = 0; y < dh; y++) for (let xx = 0; xx < dw; xx++) {
    const y0 = y * sh / dh, y1 = (y + 1) * sh / dh, x0 = xx * sw / dw, x1 = (xx + 1) * sw / dw;
    let r = 0, g = 0, b = 0, a = 0, n = 0;
    for (let sy = Math.floor(y0); sy < Math.ceil(y1); sy++) {
      const wy = Math.min(sy + 1, y1) - Math.max(sy, y0);
      for (let sx = Math.floor(x0); sx < Math.ceil(x1); sx++) {
        const w = wy * (Math.min(sx + 1, x1) - Math.max(sx, x0)), i = (sy * sw + sx) * 4, al = src[i + 3] / 255;
        r += src[i] * al * w; g += src[i + 1] * al * w; b += src[i + 2] * al * w; a += al * w; n += w;
      }
    }
    const o = (y * dw + xx) * 4;
    if (a > 0) { out[o] = r / a; out[o + 1] = g / a; out[o + 2] = b / a; }
    out[o + 3] = 255 * a / n;
  }
  return out;
}

async function main() {
  const { canvas, box } = await cutout();
  // The plate, alone, filling PLATE of a 1024 canvas. The drawing is a
  // hair off square (1012 by 1001); it is fitted to the square outright,
  // a stretch of one percent that no eye finds.
  // Two masters. The grid one (PLATE of S) is the Dock icon the app sets at
  // start, and the Windows and Linux icon. The bundle's .icns is the plate
  // filling the whole canvas and opaque to its corners: macOS 26 masks every
  // app icon to its own rounded square and sets a grey backing under it, so
  // anything transparent (a margin, the plate's own rounder corners, its
  // soft edge) shows as a grey border in Finder, the switcher and Spotlight.
  // The corners are filled by extending the plate's edge colour outward,
  // and the system's mask cuts them to its shape.
  const plate = (side) => {
    const c = createCanvas(S, S), x = c.getContext('2d');
    x.imageSmoothingEnabled = true; x.imageSmoothingQuality = 'high';
    const m = (S - side) / 2;
    x.drawImage(canvas, box[0], box[1], box[2], box[3], m, m, side, side);
    return c;
  };
  const big = plate(PLATE), full = plate(S);
  {
    const x = full.getContext('2d'), img = x.getImageData(0, 0, S, S), d = img.data;
    const opaque = (i) => d[i + 3] === 255;
    const copy = (to, from) => { d[to] = d[from]; d[to + 1] = d[from + 1]; d[to + 2] = d[from + 2]; d[to + 3] = 255; };
    // Every row: the first and last opaque pixel spread to the row's ends.
    for (let y = 0; y < S; y++) {
      let first = -1, last = -1;
      for (let xx = 0; xx < S; xx++) if (opaque((y * S + xx) * 4)) { if (first < 0) first = xx; last = xx; }
      if (first < 0) continue;
      for (let xx = 0; xx < first; xx++) copy((y * S + xx) * 4, (y * S + first) * 4);
      for (let xx = last + 1; xx < S; xx++) copy((y * S + xx) * 4, (y * S + last) * 4);
    }
    // Every column, for rows the plate never reached.
    for (let xx = 0; xx < S; xx++) {
      let first = -1, last = -1;
      for (let y = 0; y < S; y++) if (opaque((y * S + xx) * 4)) { if (first < 0) first = y; last = y; }
      if (first < 0) continue;
      for (let y = 0; y < first; y++) copy((y * S + xx) * 4, (first * S + xx) * 4);
      for (let y = last + 1; y < S; y++) copy((y * S + xx) * 4, (last * S + xx) * 4);
    }
    // What is left half-covered (the anti-aliased edge) sits on its own colour.
    for (let i = 0; i < d.length; i += 4) d[i + 3] = 255;
    x.putImageData(img, 0, 0);
  }
  const drawer = (source) => {
    const master = source.getContext('2d').getImageData(0, 0, S, S).data;
    return (size) => {
      const c = createCanvas(size, size);
      if (size === S) { c.getContext('2d').drawImage(source, 0, 0); return c; }
      c.getContext('2d').putImageData(new ImageData(shrink(master, S, S, size, size), size, size), 0, 0);
      return c;
    };
  };
  const draw = drawer(big), drawFull = drawer(full);

  const out = process.argv[2] || '.';
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, 'icon-1024.png'), big.toBuffer('image/png'));
  const iconset = path.join(out, 'icon.iconset');
  fs.mkdirSync(iconset, { recursive: true });
  for (const [name, size] of [['16x16', 16], ['16x16@2x', 32], ['32x32', 32], ['32x32@2x', 64], ['128x128', 128], ['128x128@2x', 256], ['256x256', 256], ['256x256@2x', 512], ['512x512', 512], ['512x512@2x', 1024]]) {
    fs.writeFileSync(path.join(iconset, `icon_${name}.png`), drawFull(size).toBuffer('image/png'));
  }
  for (const size of [32, 128, 256, 512]) {
    fs.writeFileSync(path.join(out, `icon-${size}.png`), draw(size).toBuffer('image/png'));
  }
  // ICO: a directory of PNG-compressed entries, which Windows reads since Vista.
  const sizes = [16, 24, 32, 48, 64, 128, 256];
  const pngs = sizes.map(s => draw(s).toBuffer('image/png'));
  const header = Buffer.alloc(6); header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(sizes.length, 4);
  const dir = Buffer.alloc(16 * sizes.length);
  let offset = 6 + dir.length;
  sizes.forEach((s, i) => {
    const e = i * 16;
    dir.writeUInt8(s === 256 ? 0 : s, e); dir.writeUInt8(s === 256 ? 0 : s, e + 1);
    dir.writeUInt8(0, e + 2); dir.writeUInt8(0, e + 3);
    dir.writeUInt16LE(1, e + 4); dir.writeUInt16LE(32, e + 6);
    dir.writeUInt32LE(pngs[i].length, e + 8); dir.writeUInt32LE(offset, e + 12);
    offset += pngs[i].length;
  });
  fs.writeFileSync(path.join(out, 'icon.ico'), Buffer.concat([header, dir, ...pngs]));
  console.log('icon cut from logo.png, box', box.join('x'), 'written to', out);
}
main().catch(e => { console.error(e); process.exit(1); });
