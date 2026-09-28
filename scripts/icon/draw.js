// The app icon, drawn on a canvas after the logo JP had generated: a
// terracotta plate, a cream scroll that curls at the top-left and
// bottom-left and is still rolled on the right, and a dark terminal panel
// on the open sheet with a prompt and lines of output. Same palette as the
// window (themes/emaki.json), drawn here so every size is regenerable.
//
//   node draw.js <out-dir>   writes icon-1024.png, the smaller PNGs, icon.ico
//                            and an icon.iconset/ for iconutil.
const { createCanvas } = require('@napi-rs/canvas');
const fs = require('fs'), path = require('path');

const S = 1024;
function draw(size) {
  const c = createCanvas(size, size), x = c.getContext('2d');
  x.scale(size / S, size / S);
  const rr = (x0, y0, w, h, r) => { x.beginPath(); x.roundRect(x0, y0, w, h, r); };
  const poly = (pts) => { x.beginPath(); pts.forEach(([px, py], i) => (i ? x.lineTo(px, py) : x.moveTo(px, py))); x.closePath(); };

  // Plate: terracotta, warmer at the top.
  const P = 100, PW = S - 2 * P;
  x.save();
  x.shadowColor = 'rgba(0,0,0,0.18)'; x.shadowBlur = 40; x.shadowOffsetY = 16;
  rr(P, P, PW, PW, 190); x.fillStyle = '#DF5A32'; x.fill();
  x.restore();
  const pg = x.createLinearGradient(0, P, 0, P + PW);
  pg.addColorStop(0, '#E9613A'); pg.addColorStop(1, '#D74A22');
  rr(P, P, PW, PW, 190); x.fillStyle = pg; x.fill();

  // The sheet's frame: a parallelogram, a touch wider at the bottom right,
  // so the panel reads as lying on a surface rather than pasted flat.
  const TL = [318, 262], TR = [712, 256], BL = [290, 732], BR = [748, 726];
  const cream = '#F6DDBF', creamDark = '#E5BE9A', creamLight = '#FCEEDD';

  // Left margin of the sheet, curling toward the viewer at both ends.
  x.save(); x.shadowColor = 'rgba(60,20,5,0.30)'; x.shadowBlur = 26; x.shadowOffsetY = 12;
  poly([[236, 262], TL, BL, [232, 716]]); x.fillStyle = cream; x.fill();
  x.restore();
  const lg = x.createLinearGradient(236, 0, 330, 0);
  lg.addColorStop(0, creamDark); lg.addColorStop(0.5, creamLight); lg.addColorStop(1, cream);
  poly([[236, 262], TL, BL, [232, 716]]); x.fillStyle = lg; x.fill();
  // A curl: the sheet's edge rolled over toward the viewer. A lit cylinder
  // end with the roll's hollow as a darker crescent on the side it opens.
  const curl = (cx, cy, r, open) => {
    x.save(); x.shadowColor = 'rgba(60,20,5,0.25)'; x.shadowBlur = 16; x.shadowOffsetY = 6;
    const g = x.createLinearGradient(cx - r, cy - r, cx + r, cy + r);
    g.addColorStop(0, creamLight); g.addColorStop(0.6, cream); g.addColorStop(1, creamDark);
    x.beginPath(); x.arc(cx, cy, r, 0, Math.PI * 2); x.fillStyle = g; x.fill(); x.restore();
    const hg = x.createRadialGradient(cx + open * r * 0.3, cy, r * 0.05, cx + open * r * 0.3, cy, r * 0.7);
    hg.addColorStop(0, '#B98A66'); hg.addColorStop(0.7, '#D6AE88'); hg.addColorStop(1, creamDark);
    x.beginPath(); x.arc(cx + open * r * 0.3, cy, r * 0.66, 0, Math.PI * 2); x.fillStyle = hg; x.fill();
    x.beginPath(); x.arc(cx + open * r * 0.3, cy, r * 0.42, Math.PI * 0.2, Math.PI * 1.9); x.strokeStyle = 'rgba(110,60,30,0.35)'; x.lineWidth = 5; x.stroke();
  };
  // The curls are the sheet's top and bottom edges rolled toward the
  // viewer: a short horizontal cylinder, lit from above, whose left end
  // shows the spiral.
  const hcurl = (x0, y0, w, h, end) => {
    x.save(); x.shadowColor = 'rgba(60,20,5,0.28)'; x.shadowBlur = 18; x.shadowOffsetY = 8;
    const g = x.createLinearGradient(0, y0, 0, y0 + h);
    g.addColorStop(0, creamLight); g.addColorStop(0.55, cream); g.addColorStop(1, '#D3A883');
    rr(x0, y0, w, h, h / 2); x.fillStyle = g; x.fill(); x.restore();
    curl(x0 + h / 2, y0 + h / 2, h / 2, end);
  };
  hcurl(204, 224, 130, 92, 1);
  hcurl(212, 660, 122, 86, 1);

  // The open sheet under the panel: a little wider than the panel on every
  // side, with the panel's own shadow on it.
  x.save(); x.shadowColor = 'rgba(60,20,5,0.28)'; x.shadowBlur = 30; x.shadowOffsetY = 14;
  poly([[TL[0] - 18, TL[1] - 8], [TR[0] + 30, TR[1] - 8], [BR[0] + 34, BR[1] + 8], [BL[0] - 18, BL[1] + 8]]);
  x.fillStyle = cream; x.fill();
  x.restore();

  // The dark panel, drawn in its own sheared frame so the lines follow it.
  const w = TR[0] - TL[0], h = BL[1] - TL[1];
  const a = 1, b = (TR[1] - TL[1]) / w, cc = (BL[0] - TL[0]) / h, d = 1;
  x.save();
  x.transform(a, b, cc, d, TL[0], TL[1]);
  x.save(); x.shadowColor = 'rgba(0,0,0,0.35)'; x.shadowBlur = 24; x.shadowOffsetY = 10;
  rr(0, 0, w, h, 26); x.fillStyle = '#23252F'; x.fill(); x.restore();
  const eg = x.createLinearGradient(0, 0, 0, h);
  eg.addColorStop(0, '#2B2E3A'); eg.addColorStop(1, '#1E2029');
  rr(0, 0, w, h, 26); x.fillStyle = eg; x.fill();
  rr(2, 2, w - 4, h - 4, 24); x.strokeStyle = 'rgba(255,255,255,0.07)'; x.lineWidth = 3; x.stroke();

  // The prompt: a chevron and an underscore in cream.
  x.strokeStyle = '#F6DDBF'; x.lineWidth = 22; x.lineCap = 'round'; x.lineJoin = 'round';
  x.beginPath(); x.moveTo(52, 58); x.lineTo(96, 92); x.lineTo(52, 126); x.stroke();
  x.beginPath(); x.moveTo(120, 132); x.lineTo(172, 132); x.stroke();

  // Lines of output: grey, blue, green, and again.
  const bar = (bx, by, bw, col) => { rr(bx, by, bw, 24, 12); x.fillStyle = col; x.fill(); };
  const grey = '#6C6E7A', blue = '#3E7BE4', green = '#38B57C';
  const rows = [[grey, 172], [blue, 320], [green, 192], [grey, 150], [blue, 300], [green, 150]];
  rows.forEach(([col, bw], i) => bar(44, 182 + i * 42, bw, col));
  x.restore();

  // The roll on the right: a cylinder from the top of the sheet to below it,
  // lit from the left, with the wound end showing at the top.
  const rx = 716, rw = 136, ry0 = 236, ry1 = 762;
  x.save(); x.shadowColor = 'rgba(60,20,5,0.32)'; x.shadowBlur = 30; x.shadowOffsetX = -10; x.shadowOffsetY = 12;
  rr(rx, ry0, rw, ry1 - ry0, 68); x.fillStyle = cream; x.fill(); x.restore();
  const rg = x.createLinearGradient(rx, 0, rx + rw, 0);
  rg.addColorStop(0, creamDark); rg.addColorStop(0.3, creamLight); rg.addColorStop(0.75, cream); rg.addColorStop(1, '#D8AE88');
  rr(rx, ry0, rw, ry1 - ry0, 68); x.fillStyle = rg; x.fill();
  // The roll's top just rounds off with a highlight; at the bottom the
  // sheet's edge curls under, toward the viewer.
  x.save(); x.translate(rx + rw / 2, ry0 + 24); x.scale(1, 0.42);
  x.beginPath(); x.arc(0, 0, rw / 2 - 6, 0, Math.PI * 2); x.fillStyle = creamLight; x.fill(); x.restore();
  hcurl(rx - 4, ry1 - 92, rw + 10, 88, -1);
  return c;
}

const out = process.argv[2] || '.';
fs.mkdirSync(out, { recursive: true });
const big = draw(1024);
fs.writeFileSync(path.join(out, 'icon-1024.png'), big.toBuffer('image/png'));
// Each size is drawn at that size, not resampled, so edges stay crisp.
const iconset = path.join(out, 'icon.iconset');
fs.mkdirSync(iconset, { recursive: true });
for (const [name, size] of [['16x16', 16], ['16x16@2x', 32], ['32x32', 32], ['32x32@2x', 64], ['128x128', 128], ['128x128@2x', 256], ['256x256', 256], ['256x256@2x', 512], ['512x512', 512], ['512x512@2x', 1024]]) {
  fs.writeFileSync(path.join(iconset, `icon_${name}.png`), draw(size).toBuffer('image/png'));
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
console.log('icon written to', out);
