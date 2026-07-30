const { execFileSync } = require('child_process');

const actionPath = __dirname;
const outLink = process.env['INPUT_OUT-LINK'] || 'result';
const save = process.env.INPUT_SAVE || 'true';
const force = process.env['INPUT_FORCE'] === 'true';

if (save !== 'true') {
  console.log('Skipping cache upload (save=false)');
  process.exit(0);
}

const args = ['run', 'nixcache-upload', '--out-link', outLink];
if (force) args.push('--force');

try {
  execFileSync('uv', args, {
    cwd: actionPath,
    stdio: 'inherit',
  });
} catch (e) {
  console.log('::warning::Cache upload failed: ' + (e.message || 'unknown error'));
}
