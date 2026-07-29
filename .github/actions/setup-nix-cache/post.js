const { execFileSync } = require('child_process');

const actionPath = __dirname;
const outLink = process.env.INPUT_OUT_LINK || 'result';
const save = process.env.INPUT_SAVE || 'true';

if (save !== 'true') {
  console.log('Skipping cache upload (save=false)');
  process.exit(0);
}

try {
  execFileSync('uv', ['run', 'nixcache-upload', '--out-link', outLink], {
    cwd: actionPath,
    stdio: 'inherit',
  });
} catch (e) {
  console.log('::warning::Cache upload failed: ' + (e.message || 'unknown error'));
}
