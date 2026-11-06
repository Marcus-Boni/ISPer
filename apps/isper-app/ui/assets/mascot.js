// O mascote do assistente: o ponto de gravação da marca do ISPer, vivo.
// Uma forma fluida e luminosa (shader próprio sobre three.js) com um rosto
// mínimo, nas cores da marca, que segue o que o assistente está fazendo.
//
//   const m = new Mascot(canvas, { labels });  // labels: estado → texto (senão, decorativo)
//   m.set('thinking');   // idle · listening · thinking · speaking · confirm · done · offline
//   m.level(0.4);        // o nível da voz de quem fala (0–1) enquanto ele ouve
//   m.poke();            // a cutucada (o clique no canvas já chama)
//   m.advance(1.5);      // avança a simulação sem esperar o relógio (testes, prints)
//   m.dispose();
//
// Princípios:
// - A forma é uma superfície só, deformada por ruído no shader (nada de
//   esferas coladas); a cor é o gradiente da marca, com brilho na borda.
// - Ouvindo, a superfície ondula com a sua voz, como a onda do logo.
// - Tudo anima por molas: trocar de estado no meio continua do ponto em
//   que está. Cada estado tem uma intenção legível em 40 px.
// - A comemoração é rara de propósito (só no fechamento do dia).
// - "Reduzir movimento": só a pose muda; sem ondas, giro, pulo nem
//   faíscas; o laço para quando nada muda.
// - Só desenha à vista e com a aba ativa; menos geometria no tamanho
//   pequeno; perde e recupera o contexto do WebGL sem quebrar.
// three.js 0.170.0 embutido (vendor/, licença MIT em vendor/three.LICENSE):
// a CSP só aceita o que vem do próprio app.
import * as THREE from './vendor/three.module.min.js';

const C = {
  deep: 0xb0443b,    // terracota fundo (a base do gradiente)
  terra: 0xf07e72,   // terracota da marca
  peach: 0xf9b6a6,   // o topo iluminado
  rim: 0xffe2d4,     // o brilho da borda
  ink: 0x1b0f0d,     // olhos e boca
  ivory: 0xece7e1,
  amber: 0xe8c15a,
};

// Mola amortecida. `c` = 2·√k é o amortecimento crítico.
class Spring {
  constructor(value = 0, k = 170, c = 22) {
    this.value = value;
    this.target = value;
    this.vel = 0;
    this.k = k;
    this.c = c;
  }
  step(dt) {
    const a = this.k * (this.target - this.value) - this.c * this.vel;
    this.vel += a * dt;
    this.value += this.vel * dt;
  }
  snap() {
    this.value = this.target;
    this.vel = 0;
  }
  get settled() {
    return Math.abs(this.target - this.value) < 1e-3 && Math.abs(this.vel) < 1e-3;
  }
}

const clamp01 = (v) => Math.max(0, Math.min(1, v));

// As poses. `amp`/`speed`: o quanto a superfície se move. `wave`: ondas da
// voz. `swirl`: o gradiente girando. `glow`: o halo. `sat`: a cor (0 = cinza).
// `eyes`: abertura (1 normal). `look*`: para onde os olhos vão. `turn`/`nod`:
// o rosto para o lado e para cima (-) ou baixo (+). `happy`: olhos ^ ^.
// `mouth`: boca aberta. `talk`: boca no ritmo da fala. `dots`: as bolinhas
// em órbita.
const BASE = {
  amp: 0.018, speed: 0.32, wave: 0, swirl: 0, glow: 0.25, sat: 1,
  eyes: 1, lookX: 0, lookY: 0, turn: 0, nod: 0, tilt: 0, happy: 0, mouth: 0, talk: 0, dots: 0,
};
const POSES = {
  idle: { ...BASE },
  // Atento: rosto de lado, olhos maiores, a superfície ondulando com a voz.
  listening: { ...BASE, amp: 0.03, wave: 1, glow: 0.55, eyes: 1.14, tilt: 0.16, nod: -0.04 },
  // Procurando a resposta: olha para cima e para o lado, o gradiente gira,
  // três bolinhas em órbita.
  thinking: { ...BASE, amp: 0.05, speed: 0.8, swirl: 1, glow: 0.4, eyes: 0.9, lookX: 0.9, lookY: 0.9, turn: 0.22, nod: -0.16, dots: 1 },
  // A resposta chegando: a boca no ritmo e a superfície pulsando junto.
  speaking: { ...BASE, amp: 0.04, speed: 0.55, glow: 0.45, eyes: 1.02, nod: -0.03, talk: 1 },
  // "É com você": olha para baixo, onde o cartão está, e chama com o brilho.
  confirm: { ...BASE, glow: 0.7, eyes: 1.08, lookX: 0.35, lookY: -0.75, nod: 0.12, turn: 0.1 },
  // Olhos ^ ^, sorriso, o pulo e as faíscas.
  done: { ...BASE, amp: 0.05, speed: 0.6, glow: 0.8, happy: 1, mouth: 0.7, nod: -0.08 },
  // Sem conexão: quase sem cor, olhos meio fechados, olhando para baixo.
  offline: { ...BASE, amp: 0.015, speed: 0.15, glow: 0, sat: 0.28, eyes: 0.4, lookY: -0.6, nod: 0.18, tilt: 0.1 },
};
const HOP = { confirm: 1.6, done: 3.0 };
const CROUCH = 0.11;
const ORB_Y = 1.05;

// Ruído simplex 3D (Ashima Arts / Stefan Gustavson, licença MIT).
const NOISE = /* glsl */ `
vec3 mod289(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec4 mod289(vec4 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec4 permute(vec4 x) { return mod289(((x * 34.0) + 10.0) * x); }
vec4 taylorInvSqrt(vec4 r) { return 1.79284291400159 - 0.85373472095314 * r; }
float snoise(vec3 v) {
  const vec2 C = vec2(1.0 / 6.0, 1.0 / 3.0);
  const vec4 D = vec4(0.0, 0.5, 1.0, 2.0);
  vec3 i = floor(v + dot(v, C.yyy));
  vec3 x0 = v - i + dot(i, C.xxx);
  vec3 g = step(x0.yzx, x0.xyz);
  vec3 l = 1.0 - g;
  vec3 i1 = min(g.xyz, l.zxy);
  vec3 i2 = max(g.xyz, l.zxy);
  vec3 x1 = x0 - i1 + C.xxx;
  vec3 x2 = x0 - i2 + C.yyy;
  vec3 x3 = x0 - D.yyy;
  i = mod289(i);
  vec4 p = permute(permute(permute(
      i.z + vec4(0.0, i1.z, i2.z, 1.0))
    + i.y + vec4(0.0, i1.y, i2.y, 1.0))
    + i.x + vec4(0.0, i1.x, i2.x, 1.0));
  float n_ = 0.142857142857;
  vec3 ns = n_ * D.wyz - D.xzx;
  vec4 j = p - 49.0 * floor(p * ns.z * ns.z);
  vec4 x_ = floor(j * ns.z);
  vec4 y_ = floor(j - 7.0 * x_);
  vec4 x = x_ * ns.x + ns.yyyy;
  vec4 y = y_ * ns.x + ns.yyyy;
  vec4 h = 1.0 - abs(x) - abs(y);
  vec4 b0 = vec4(x.xy, y.xy);
  vec4 b1 = vec4(x.zw, y.zw);
  vec4 s0 = floor(b0) * 2.0 + 1.0;
  vec4 s1 = floor(b1) * 2.0 + 1.0;
  vec4 sh = -step(h, vec4(0.0));
  vec4 a0 = b0.xzyw + s0.xzyw * sh.xxyy;
  vec4 a1 = b1.xzyw + s1.xzyw * sh.zzww;
  vec3 p0 = vec3(a0.xy, h.x);
  vec3 p1 = vec3(a0.zw, h.y);
  vec3 p2 = vec3(a1.xy, h.z);
  vec3 p3 = vec3(a1.zw, h.w);
  vec4 norm = taylorInvSqrt(vec4(dot(p0, p0), dot(p1, p1), dot(p2, p2), dot(p3, p3)));
  p0 *= norm.x; p1 *= norm.y; p2 *= norm.z; p3 *= norm.w;
  vec4 m = max(0.6 - vec4(dot(x0, x0), dot(x1, x1), dot(x2, x2), dot(x3, x3)), 0.0);
  m = m * m;
  return 42.0 * dot(m * m, vec4(dot(p0, x0), dot(p1, x1), dot(p2, x2), dot(p3, x3)));
}
`;

const ORB_VERTEX = /* glsl */ `
uniform float uTime;
uniform float uAmp;
uniform float uWave;
uniform float uVoice;
uniform float uPhase;
varying vec3 vNormal;
varying vec3 vView;
varying vec3 vPos;
varying float vDisp;
${NOISE}
// O deslocamento de um ponto da esfera: duas oitavas de ruído e, ouvindo,
// anéis que sobem com a voz (a onda do logo, em volta).
float disp(vec3 p) {
  // Ruído largo e baixo: a forma respira sem virar "caroço".
  float d = snoise(p * 1.15 + vec3(0.0, uPhase, uPhase * 0.6)) * uAmp;
  d += snoise(p * 2.2 - vec3(uPhase * 1.3)) * uAmp * 0.15;
  d += sin(p.y * 14.0 - uTime * 9.0) * uWave * uVoice * 0.026 * (1.0 - abs(p.y));
  return d;
}
vec3 displaced(vec3 dir) { return dir * (1.0 + disp(dir)); }
void main() {
  vec3 dir = normalize(position);
  vec3 p = displaced(dir);
  // A normal pela diferença finita (a superfície muda a cada quadro).
  vec3 t = normalize(cross(dir, abs(dir.y) < 0.99 ? vec3(0.0, 1.0, 0.0) : vec3(1.0, 0.0, 0.0)));
  vec3 b = normalize(cross(dir, t));
  float e = 0.012;
  vec3 pt = displaced(normalize(dir + t * e));
  vec3 pb = displaced(normalize(dir + b * e));
  vec3 n = normalize(cross(pt - p, pb - p));
  if (dot(n, dir) < 0.0) n = -n;
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  vNormal = normalize(normalMatrix * n);
  vView = normalize(-mv.xyz);
  vPos = p;
  vDisp = disp(dir);
  gl_Position = projectionMatrix * mv;
}
`;

const ORB_FRAGMENT = /* glsl */ `
uniform float uPhase;
uniform vec3 uDeep;
uniform vec3 uTerra;
uniform vec3 uPeach;
uniform vec3 uRim;
uniform float uSwirl;
uniform float uGlow;
uniform float uSat;
varying vec3 vNormal;
varying vec3 vView;
varying vec3 vPos;
varying float vDisp;
${NOISE}
void main() {
  vec3 n = normalize(vNormal);
  vec3 v = normalize(vView);
  float facing = max(dot(n, v), 0.0);
  float edge = 1.0 - facing;
  // O gradiente da marca na diagonal (terracota fundo embaixo à esquerda,
  // pêssego no alto à direita), torcido por dois fluxos lentos em sentidos
  // diferentes; gira no "pensando".
  float f1 = snoise(vPos * 0.95 + vec3(uPhase * 0.7, uPhase * 0.4, 0.0));
  float f2 = snoise(vPos * 1.7 - vec3(0.0, uPhase * 0.6, uPhase * 0.5));
  float flow = f1 * 0.62 + f2 * 0.38;
  float a = uSwirl;
  vec2 q = mat2(cos(a), -sin(a), sin(a), cos(a)) * vPos.xy;
  float g = smoothstep(-1.2, 1.1, q.y * 0.75 + q.x * 0.45 + flow * 0.5);
  vec3 base = mix(uDeep, uTerra, smoothstep(0.05, 0.55, g));
  base = mix(base, uPeach, smoothstep(0.5, 1.0, g));
  // Bandas de luz que fluem por dentro: o que dá a sensação de vivo.
  base = mix(base, uRim, smoothstep(0.3, 0.85, flow) * 0.3);
  // Luz quase de dentro para fora (pouca sombra), o reflexo nítido de
  // verniz, um brilho largo; a borda escurece e acende num fio fino, como
  // vidro.
  vec3 L = normalize(vec3(0.45, 0.85, 0.6));
  float diff = dot(n, L) * 0.5 + 0.5;
  vec3 H = normalize(L + v);
  float ndh = max(dot(n, H), 0.0);
  float spec = pow(ndh, 160.0) * 0.9 + pow(ndh, 18.0) * 0.08;
  vec3 col = base * (0.78 + 0.32 * diff);
  col *= mix(1.0, 0.72, pow(edge, 1.6));
  col += vec3(1.0, 0.97, 0.93) * spec;
  col += mix(uPeach, uRim, 0.7) * pow(edge, 5.0) * (0.9 + uGlow * 1.2);
  float lum = dot(col, vec3(0.299, 0.587, 0.114));
  col = mix(vec3(lum), col, uSat);
  gl_FragColor = vec4(col, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}
`;

const HALO_VERTEX = /* glsl */ `
varying vec2 vUv;
void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }
`;
const HALO_FRAGMENT = /* glsl */ `
uniform vec3 uColor;
uniform float uStrength;
varying vec2 vUv;
void main() {
  float d = length(vUv - 0.5) * 2.0;
  float a = pow(smoothstep(1.0, 0.0, d), 1.5) * uStrength;
  gl_FragColor = vec4(uColor, a); // a mistura aditiva já multiplica pela opacidade
  #include <colorspace_fragment>
}
`;

// Sombra de contato: um degradê radial no chão.
function contactShadow() {
  const c = document.createElement('canvas');
  c.width = c.height = 128;
  const g = c.getContext('2d');
  const grad = g.createRadialGradient(64, 64, 0, 64, 64, 64);
  grad.addColorStop(0, 'rgba(10,6,5,0.5)');
  grad.addColorStop(0.6, 'rgba(10,6,5,0.12)');
  grad.addColorStop(1, 'rgba(10,6,5,0)');
  g.fillStyle = grad;
  g.fillRect(0, 0, 128, 128);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const mesh = new THREE.Mesh(
    new THREE.PlaneGeometry(2.2, 2.2),
    new THREE.MeshBasicMaterial({ map: tex, transparent: true, depthWrite: false }),
  );
  mesh.rotation.x = -Math.PI / 2;
  return mesh;
}

const Z = new THREE.Vector3(0, 0, 1);

export class Mascot {
  constructor(canvas, { reducedMotion, labels = null, interactive = true } = {}) {
    this.canvas = canvas;
    this.reduce = reducedMotion ?? matchMedia('(prefers-reduced-motion: reduce)').matches;
    this.labels = labels;
    this.state = 'idle';
    this.pose = POSES.idle;
    this.voice = 0;
    this.voiceS = 0;
    this.time = 0;
    this.phase = 0;
    this.swirl = 0;
    this.orbit = 0;
    this.blinkAt = 1.5;
    this.blink = 1;
    this.idleIn = 3;
    this.act = null;
    this.flash = 0;
    this.pending = null;
    this.pointer = { x: 0, y: 0 };
    this.small = Math.min(canvas.clientWidth, canvas.clientHeight) < 160;

    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: 'low-power' });
    renderer.setPixelRatio(Math.min(devicePixelRatio || 1, 2));
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.NeutralToneMapping;
    this.renderer = renderer;

    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(28, 1, 0.1, 50);
    this.camera.position.set(0, 1.25, 5.6);
    this.camera.lookAt(0, ORB_Y - 0.05, 0);

    this.build();

    const s = (v, k, c) => new Spring(v, k, c);
    this.sp = {};
    for (const [k, v] of Object.entries(BASE)) this.sp[k] = s(v, 120, 20);
    Object.assign(this.sp, {
      eyes: s(1, 260, 26), happy: s(0, 200, 26), mouth: s(0, 220, 22), lookX: s(0, 140, 22), lookY: s(0, 140, 22),
      talk: s(0, 60, 14), dots: s(0, 110, 18), yaw: s(0, 60, 14), pitch: s(0, 60, 14),
      // Física: esmagar com balanço, a entrada e um tremor da superfície.
      squash: s(0, 320, 13), appear: s(this.reduce ? 1 : 0.8, 140, 15), wobble: s(0, 90, 6),
    });
    this.sp.appear.target = 1;
    this.hop = { y: 0, vel: 0 };

    this.onPointer = (e) => {
      const r = canvas.getBoundingClientRect();
      const span = Math.max(innerWidth, innerHeight) / 2;
      this.pointer.x = Math.max(-1, Math.min(1, (e.clientX - (r.left + r.width / 2)) / span));
      this.pointer.y = Math.max(-1, Math.min(1, (e.clientY - (r.top + r.height * 0.45)) / span));
    };
    this.onLeave = (e) => { if (!e.relatedTarget) this.pointer = { x: 0, y: 0 }; };
    addEventListener('pointermove', this.onPointer, { passive: true });
    document.addEventListener('mouseout', this.onLeave);
    this.onPoke = () => this.poke();
    if (interactive) canvas.addEventListener('pointerdown', this.onPoke);

    this.onLost = () => { this.lost = true; cancelAnimationFrame(this.raf); this.raf = 0; };
    this.onRestored = () => { this.lost = false; this.wake(); };
    canvas.addEventListener('webglcontextlost', this.onLost);
    canvas.addEventListener('webglcontextrestored', this.onRestored);

    this.label();
    this.resize();
    this.ro = new ResizeObserver(() => this.resize());
    this.ro.observe(canvas);
    this.visible = true;
    this.io = new IntersectionObserver(([e]) => { this.visible = e.isIntersecting; this.wake(); });
    this.io.observe(canvas);
    this.onVis = () => this.wake();
    document.addEventListener('visibilitychange', this.onVis);
    canvas.animate?.([{ opacity: 0 }, { opacity: 1 }], { duration: 260, easing: 'cubic-bezier(0.23, 1, 0.32, 1)' });

    this.last = performance.now();
    this.raf = 0;
    this.wake();
  }

  build() {
    const color = (hex) => new THREE.Color(hex);
    const root = new THREE.Group();     // flutuar, pulo
    const orb = new THREE.Group();      // esmagar, a entrada
    root.add(orb);
    orb.position.y = ORB_Y;
    this.scene.add(root);
    Object.assign(this, { root, orb });

    this.orbUniforms = {
      uTime: { value: 0 }, uPhase: { value: 0 }, uAmp: { value: BASE.amp }, uWave: { value: 0 }, uVoice: { value: 0 },
      uSwirl: { value: 0 }, uGlow: { value: BASE.glow }, uSat: { value: 1 },
      uDeep: { value: color(C.deep) }, uTerra: { value: color(C.terra) }, uPeach: { value: color(C.peach) }, uRim: { value: color(C.rim) },
    };
    const body = new THREE.Mesh(
      new THREE.IcosahedronGeometry(1, this.small ? 20 : 64),
      new THREE.ShaderMaterial({ uniforms: this.orbUniforms, vertexShader: ORB_VERTEX, fragmentShader: ORB_FRAGMENT }),
    );
    orb.add(body);

    // O halo por trás: luz somada, mais forte ouvindo e na comemoração.
    this.haloUniforms = { uColor: { value: color(C.terra) }, uStrength: { value: 0.3 } };
    const halo = new THREE.Mesh(
      new THREE.PlaneGeometry(3.3, 3.3),
      new THREE.ShaderMaterial({
        uniforms: this.haloUniforms, vertexShader: HALO_VERTEX, fragmentShader: HALO_FRAGMENT,
        transparent: true, depthWrite: false, blending: THREE.AdditiveBlending,
      }),
    );
    halo.position.set(0, ORB_Y, -0.9);
    halo.renderOrder = -1;
    root.add(halo);
    this.halo = halo;

    // O rosto: olhos em pílula (abertos) e em arco (^ ^), a boca. Ficam
    // sobre a superfície e deslizam nela quando ele olha para os lados.
    const ink = new THREE.MeshBasicMaterial({ color: C.ink, depthTest: false });
    const face = new THREE.Group();
    orb.add(face);
    this.eyes = [-1, 1].map((side) => {
      const g = new THREE.Group();
      const open = new THREE.Mesh(new THREE.CapsuleGeometry(0.068, 0.11, 6, 18), ink);
      const arc = new THREE.Mesh(new THREE.TorusGeometry(0.075, 0.024, 10, 28, Math.PI), ink);
      open.renderOrder = arc.renderOrder = 2;
      g.add(open, arc);
      face.add(g);
      return { g, open, arc, side };
    });
    this.smile = new THREE.Mesh(new THREE.TorusGeometry(0.07, 0.02, 10, 24, Math.PI), ink);
    this.smile.rotation.z = Math.PI;
    this.mouthOpen = new THREE.Mesh(new THREE.SphereGeometry(0.07, 24, 12), ink);
    this.smile.renderOrder = this.mouthOpen.renderOrder = 2;
    const mouth = new THREE.Group();
    mouth.add(this.smile, this.mouthOpen);
    face.add(mouth);
    Object.assign(this, { face, mouth });

    // As bolinhas em órbita do "pensando" (passam por trás da esfera).
    const dotMat = new THREE.MeshBasicMaterial({ color: C.peach });
    this.dots = [0, 1, 2].map(() => {
      const d = new THREE.Mesh(new THREE.SphereGeometry(0.075, 16, 10), dotMat);
      root.add(d);
      return d;
    });

    this.shadow = contactShadow();
    this.scene.add(this.shadow);

    // Faíscas da comemoração, nas cores da marca.
    const n = 16;
    const mesh = new THREE.InstancedMesh(new THREE.OctahedronGeometry(0.065), new THREE.MeshBasicMaterial(), n);
    const pal = [C.ivory, C.terra, C.peach, C.amber];
    const c = new THREE.Color();
    for (let i = 0; i < n; i++) mesh.setColorAt(i, c.setHex(pal[i % pal.length]));
    mesh.visible = false;
    mesh.frustumCulled = false;
    this.scene.add(mesh);
    this.sparks = { mesh, parts: [], tmp: new THREE.Object3D() };
  }

  burst() {
    const k = this.sparks;
    k.parts = Array.from({ length: k.mesh.count }, (_, i) => {
      const a = (i / k.mesh.count) * Math.PI * 2 + Math.random() * 0.4;
      const out = 1.6 + Math.random() * 1.2;
      return {
        p: new THREE.Vector3(Math.cos(a) * 0.9, ORB_Y + Math.sin(a) * 0.9, 0.4),
        v: new THREE.Vector3(Math.cos(a) * out, Math.sin(a) * out + 1.0, 0.3),
        life: 0.9 + Math.random() * 0.5,
        spin: 4 + Math.random() * 6,
      };
    });
    k.mesh.visible = true;
  }

  stepSparks(dt) {
    const k = this.sparks;
    if (!k.parts.length) return;
    let alive = 0;
    k.parts.forEach((q, i) => {
      q.life -= dt;
      q.v.y -= 4 * dt;
      q.v.multiplyScalar(1 - 2.2 * dt);
      q.p.addScaledVector(q.v, dt);
      const s = q.life > 0 ? Math.min(1, q.life / 0.4) : 0;
      if (s > 0) alive++;
      k.tmp.position.copy(q.p);
      k.tmp.rotation.set(0, q.spin * q.life, q.spin * q.life * 0.5);
      k.tmp.scale.setScalar(s);
      k.tmp.updateMatrix();
      k.mesh.setMatrixAt(i, k.tmp.matrix);
    });
    k.mesh.instanceMatrix.needsUpdate = true;
    if (!alive) { k.parts = []; k.mesh.visible = false; }
  }

  set(state) {
    if (!POSES[state] || state === this.state) return;
    this.state = state;
    this.pose = POSES[state];
    this.act = null;
    if (!this.reduce && HOP[state] && this.hop.y < 0.01) this.pending = { v: HOP[state], t: CROUCH };
    if (!this.reduce && state === 'done') this.burstAt = CROUCH + 0.2;
    if (!this.reduce) this.sp.wobble.vel += 1.2;
    this.label();
    this.wake();
  }

  level(v) {
    this.voice = clamp01(v || 0);
    if (this.voice) this.wake();
  }

  // A cutucada: resposta imediata (amassa, a superfície treme, sorri).
  poke() {
    this.flash = 0.7;
    if (!this.reduce) {
      this.sp.squash.vel += 4;
      this.sp.wobble.vel += 3;
      if (this.hop.y < 0.01 && !this.pending) this.hop.vel = 1.2;
    }
    this.wake();
  }

  label() {
    // O estado à vista no DOM (testes e2e e CSS).
    this.canvas.dataset.state = this.state;
    const text = this.labels && this.labels[this.state];
    if (text) {
      this.canvas.setAttribute('role', 'img');
      this.canvas.setAttribute('aria-label', text);
      this.canvas.removeAttribute('aria-hidden');
    } else {
      this.canvas.setAttribute('aria-hidden', 'true');
    }
  }

  advance(seconds) {
    for (let t = 0; t < seconds; t += 1 / 60) {
      this.time += 1 / 60;
      this.apply(1 / 60);
    }
    this.renderer.render(this.scene, this.camera);
  }

  resize() {
    const w = this.canvas.clientWidth || 1;
    const h = this.canvas.clientHeight || 1;
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.apply(0);
    this.renderer.render(this.scene, this.camera);
  }

  wake() {
    if (this.raf || this.lost || !this.visible || document.hidden) return;
    this.last = performance.now();
    this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  frame(now) {
    this.raf = 0;
    if (this.lost || !this.visible || document.hidden) return;
    const dt = Math.min((now - this.last) / 1000, 1 / 30);
    this.last = now;
    this.time += dt;
    this.apply(dt);
    this.renderer.render(this.scene, this.camera);
    const still = this.reduce && this.flash <= 0 && !this.sparks.parts.length && Object.values(this.sp).every((s) => s.settled);
    if (!still) this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  // Parado, de vez em quando: olhar de lado, olhar para cima, um tremor.
  idleActs(dt) {
    const over = {};
    if (this.reduce || this.state !== 'idle') { this.act = null; return over; }
    if (!this.act) {
      this.idleIn -= dt;
      if (this.idleIn > 0) return over;
      const kinds = ['glance', 'up', 'wobble'].filter((k) => k !== this.lastAct);
      const kind = kinds[Math.floor(Math.random() * kinds.length)];
      this.act = { kind, side: Math.random() < 0.5 ? -1 : 1, t: 0, dur: { glance: 1.2, up: 1.0, wobble: 0.1 }[kind] };
      this.lastAct = kind;
      this.idleIn = 3.5 + Math.random() * 4;
      if (kind === 'wobble') this.sp.wobble.vel += 2;
    }
    const a = this.act;
    a.t += dt;
    if (a.kind === 'glance') { over.lookX = a.side * 0.85; over.turn = a.side * 0.12; }
    if (a.kind === 'up') { over.lookY = 0.7; over.nod = -0.08; }
    if (a.t >= a.dur) this.act = null;
    return over;
  }

  apply(dt) {
    const sp = this.sp;
    const calm = this.reduce;
    const over = this.idleActs(dt);
    this.flash = Math.max(0, this.flash - dt);
    if (this.flash > 0) { over.happy = 1; over.mouth = 0.45; }
    for (const k of Object.keys(BASE)) sp[k].target = this.pose[k] + (over[k] || 0);
    sp.happy.target = Math.max(this.pose.happy, over.happy || 0);
    sp.mouth.target = Math.max(this.pose.mouth, over.mouth || 0);

    const follow = !calm && (this.state === 'idle' || this.state === 'listening');
    sp.yaw.target = follow ? this.pointer.x * 0.35 : 0;
    sp.pitch.target = follow ? this.pointer.y * 0.18 : 0;

    // Pulo: agacha, sai, cai; ao tocar o chão, esmaga e a superfície treme.
    const h = this.hop;
    if (this.pending) {
      this.pending.t -= dt;
      sp.squash.target = 0.12;
      if (this.pending.t <= 0) {
        h.vel = this.pending.v;
        sp.squash.target = 0;
        sp.squash.vel -= 2;
        this.pending = null;
      }
    }
    if (h.y > 0 || h.vel > 0) {
      h.vel -= 12 * dt;
      h.y += h.vel * dt;
      if (h.y <= 0) {
        const impact = Math.min(1, -h.vel / 4);
        h.y = 0;
        h.vel = 0;
        sp.squash.vel += 3 * impact;
        sp.wobble.vel += 2.5 * impact;
      }
    }
    if (this.burstAt != null) {
      this.burstAt -= dt;
      if (this.burstAt <= 0) { this.burstAt = null; this.burst(); }
    }

    for (const s of Object.values(sp)) {
      if (calm) s.snap();
      else s.step(dt);
    }
    if (calm) for (const k of ['squash', 'wobble']) { sp[k].value = 0; sp[k].vel = 0; }
    this.stepSparks(dt);

    // A superfície: a fase do ruído anda na velocidade da pose; a voz é
    // suavizada (sobe rápido, desce devagar) para as ondas não tremerem.
    this.voiceS += (this.voice - this.voiceS) * Math.min(1, dt * (this.voice > this.voiceS ? 18 : 6));
    const talk = calm ? 0 : sp.talk.value * (0.25 + 0.75 * Math.abs(Math.sin(this.time * 13) * Math.sin(this.time * 7.3)));
    if (!calm) {
      this.phase += dt * sp.speed.value;
      this.swirl += dt * sp.swirl.value * 1.4;
      this.orbit += dt * 2.4;
    }
    const u = this.orbUniforms;
    u.uTime.value = this.time;
    u.uPhase.value = this.phase;
    u.uAmp.value = calm ? 0.02 : sp.amp.value + Math.abs(sp.wobble.value) * 0.08 + talk * 0.02;
    u.uWave.value = calm ? 0 : sp.wave.value;
    u.uVoice.value = this.voiceS;
    u.uSwirl.value = this.swirl;
    u.uGlow.value = sp.glow.value + this.voiceS * sp.wave.value * 0.6;
    u.uSat.value = sp.sat.value;
    this.haloUniforms.uStrength.value = (0.3 + sp.glow.value * 0.7 + this.voiceS * sp.wave.value * 0.5) * sp.sat.value;

    // Flutuar (parado), respirar, esmagar.
    const hover = calm ? 0 : Math.sin(this.time * 1.6) * 0.04;
    const breathe = calm ? 0 : Math.sin(this.time * 2.1) * 0.01 + Math.sin(this.time * 0.73) * 0.004;
    const sq = sp.squash.value - Math.min(h.vel, 3) * 0.015;
    const appear = sp.appear.value;
    this.root.position.y = h.y + hover;
    this.orb.scale.set(appear * (1 + sq * 0.55 - breathe * 0.4), appear * (1 - sq + breathe), appear * (1 + sq * 0.55));

    // O rosto desliza sobre a esfera: direção = olhar + virar + inclinar.
    const ponder = calm || this.state !== 'thinking' ? 0 : Math.sin(this.time * 1.1) * 0.08;
    const yaw = sp.yaw.value + sp.turn.value + ponder;
    const pitch = sp.pitch.value + sp.nod.value + (calm ? 0 : sp.talk.value * Math.sin(this.time * 6.5) * 0.03);
    const tilt = sp.tilt.value;
    const look = new THREE.Euler(pitch, yaw, -tilt * 0.5, 'YXZ');
    const place = (obj, x, y, r) => {
      const dir = new THREE.Vector3(x, y, 1).normalize().applyEuler(look);
      obj.position.copy(dir).multiplyScalar(r);
      obj.quaternion.setFromUnitVectors(Z, dir);
      obj.rotateZ(-tilt);
    };

    if (!calm) {
      this.blinkAt -= dt;
      if (this.blinkAt <= 0) {
        this.blinkT = 0;
        this.blinkAt = Math.random() < 0.2 ? 0.22 : 2.6 + Math.random() * 3.4;
      }
      if (this.blinkT != null) {
        this.blinkT += dt;
        const t = this.blinkT;
        this.blink = t < 0.06 ? 1 - (t / 0.06) * 0.9 : t < 0.18 ? 0.1 + ((t - 0.06) / 0.12) * 0.9 : 1;
        if (t >= 0.18) this.blinkT = null;
      }
    } else {
      this.blink = 1;
    }

    const happy = clamp01(sp.happy.value);
    const gaze = { x: sp.lookX.value * 0.09, y: sp.lookY.value * 0.08 };
    for (const e of this.eyes) {
      place(e.g, e.side * 0.27 + gaze.x, 0.14 + gaze.y, 1.03);
      const open = Math.max(0.05, sp.eyes.value * this.blink * (1 - happy));
      e.open.scale.set(Math.min(1.15, sp.eyes.value), open, 1);
      e.open.visible = open > 0.06;
      e.arc.scale.setScalar(Math.max(0.001, happy * 1.15));
      e.arc.visible = happy > 0.02;
    }
    place(this.mouth, gaze.x * 0.5, -0.16 + gaze.y * 0.4, 1.035);
    const mouthOpen = clamp01(Math.max(sp.mouth.value, talk));
    this.mouthOpen.scale.set(1 + happy * 0.25, Math.max(0.001, mouthOpen * 0.75), 0.2);
    this.mouthOpen.visible = mouthOpen > 0.03;
    this.smile.visible = happy > 0.05 || sp.mouth.value > 0.05;
    this.smile.scale.setScalar(0.8 + happy * 0.4);

    // As bolinhas em órbita, inclinada; surgem uma depois da outra.
    this.dots.forEach((d, i) => {
      const s = clamp01((sp.dots.value - i * 0.2) / 0.6);
      const a = this.orbit + i * 0.55;
      d.position.set(Math.cos(a) * 1.32, ORB_Y + Math.sin(a) * 0.42 + 0.15, Math.sin(a) * 1.0);
      d.scale.setScalar(Math.max(0.001, s * (0.75 + i * 0.15)));
      d.visible = s > 0.01;
    });

    // A sombra encolhe e clareia com a altura.
    const lift = Math.min(1, (h.y + hover + 0.04) / 0.9);
    this.shadow.scale.setScalar(appear * (1 - lift * 0.35));
    this.shadow.material.opacity = (0.9 - lift * 0.5) * (0.4 + sp.sat.value * 0.6);
  }

  dispose() {
    cancelAnimationFrame(this.raf);
    removeEventListener('pointermove', this.onPointer);
    document.removeEventListener('mouseout', this.onLeave);
    document.removeEventListener('visibilitychange', this.onVis);
    this.canvas.removeEventListener('pointerdown', this.onPoke);
    this.canvas.removeEventListener('webglcontextlost', this.onLost);
    this.canvas.removeEventListener('webglcontextrestored', this.onRestored);
    this.ro.disconnect();
    this.io.disconnect();
    this.scene.traverse((o) => {
      if (o.geometry) o.geometry.dispose();
      if (o.material) { o.material.map?.dispose(); o.material.dispose(); }
    });
    this.renderer.dispose();
  }
}
