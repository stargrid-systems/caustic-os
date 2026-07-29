const { execSync, spawn } = require('child_process');
const { appendFileSync, writeFileSync, readFileSync } = require('fs');
const path = require('path');

const actionPath = __dirname;
const publicKey = process.env.INPUT_PUBLIC_KEY || '';
const privateKey = process.env.NIX_CACHE_PRIVATE_KEY || '';
const repo = process.env.GITHUB_REPOSITORY || '';

function ghEnv(key, value) {
  const f = process.env.GITHUB_ENV;
  if (f) appendFileSync(f, `${key}=${value}\n`);
}

function ghPath(p) {
  const f = process.env.GITHUB_PATH;
  if (f) appendFileSync(f, p + '\n');
}

if (privateKey) {
  writeFileSync('/tmp/nix-cache.sec', privateKey + '\n', { mode: 0o600 });
  ghEnv('NIXCACHE_SIGNING_KEY_FILE', '/tmp/nix-cache.sec');
} else {
  console.log('::notice::NIX_CACHE_PRIVATE_KEY is not set; cache uploads will be unsigned');
}

ghEnv('NIXCACHE_REPO', repo);
if (process.env.GITHUB_TOKEN) ghEnv('GITHUB_TOKEN', process.env.GITHUB_TOKEN);

try {
  execSync('uv --version', { stdio: 'pipe' });
} catch {
  console.log('Installing uv...');
  execSync('curl -LsSf https://astral.sh/uv/install.sh | sh', { stdio: 'inherit' });
  ghPath(path.join(process.env.HOME || '/root', '.local', 'bin'));
  process.env.PATH = path.join(process.env.HOME || '/root', '.local', 'bin') + ':' + process.env.PATH;
}

console.log('Syncing nixcache-oci project...');
execSync('uv sync --frozen', { cwd: actionPath, stdio: 'inherit' });

console.log('Starting nixcache-oci proxy...');
const proxyEnv = { ...process.env, NIXCACHE_REPO: repo };
const proxy = spawn('sh', ['-c', 'uv run nixcache-proxy > /tmp/nixcache-proxy.log 2>&1'], {
  cwd: actionPath,
  detached: true,
  stdio: 'ignore',
  env: proxyEnv,
});
proxy.unref();

let ready = false;
for (let i = 0; i < 30; i++) {
  try {
    execSync('curl -fs --max-time 2 http://127.0.0.1:37515/nix-cache-info', { stdio: 'pipe' });
    ready = true;
    break;
  } catch {
    execSync('sleep 1');
  }
}

if (!ready) {
  console.log('::warning::nixcache-oci proxy did not become ready; continuing without it');
  try { console.log(readFileSync('/tmp/nixcache-proxy.log', 'utf8')); } catch {}
}

const extraConf = [];
if (ready) {
  extraConf.push('extra-substituters = http://127.0.0.1:37515');
  extraConf.push('extra-trusted-substituters = http://127.0.0.1:37515');
}
if (publicKey) {
  extraConf.push(`extra-trusted-public-keys = ${publicKey}`);
}

let installCmd = 'curl -fsSL https://install.determinate.systems/nix | sh -s -- install --no-confirm';
for (const c of extraConf) {
  installCmd += ` --extra-conf "${c}"`;
}
console.log('Installing Determinate Nix...');
execSync(installCmd, { stdio: 'inherit' });

ghPath('/nix/var/nix/profiles/default/bin');
