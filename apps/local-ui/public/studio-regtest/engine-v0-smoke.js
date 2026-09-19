// regtest-only BHANG_ENGINE smoke test — occupy back with a labeled canvas.
// Host contract: schema/mount-api.md + load-engine.js (must export mount).
self.BHANG_ENGINE = {
  mount: function (ctx) {
    var mesh = ctx && ctx.slots && ctx.slots.back;
    if (!mesh || !mesh.material || !ctx.THREE) return;

    var size = 512;
    var c = document.createElement('canvas');
    c.width = size;
    c.height = size;
    var g = c.getContext('2d');
    g.fillStyle = '#0a1628';
    g.fillRect(0, 0, size, size);
    g.strokeStyle = '#3a6a9a';
    g.lineWidth = 8;
    g.strokeRect(16, 16, size - 32, size - 32);
    g.fillStyle = '#7ec8ff';
    g.font = 'bold 56px monospace';
    g.textAlign = 'center';
    g.textBaseline = 'middle';
    g.fillText('ENGINE v0', size / 2, size / 2 - 24);
    g.fillStyle = '#a8d4ff';
    g.font = '28px monospace';
    g.fillText('regtest smoke', size / 2, size / 2 + 36);

    var tex = new ctx.THREE.CanvasTexture(c);
    if (ctx.THREE.SRGBColorSpace !== undefined) tex.colorSpace = ctx.THREE.SRGBColorSpace;
    else if (ctx.THREE.sRGBEncoding !== undefined) tex.encoding = ctx.THREE.sRGBEncoding;
    tex.needsUpdate = true;

    if (mesh.material.map && typeof mesh.material.map.dispose === 'function') {
      mesh.material.map.dispose();
    }
    mesh.material.map = tex;
    mesh.material.transparent = true;
    mesh.material.opacity = 0.9;
    mesh.material.needsUpdate = true;

    ctx.occupy('back');
  }
};
