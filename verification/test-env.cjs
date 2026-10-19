const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..', 'source');
const outputDirectory = path.resolve(
  process.env.UM_VERIFICATION_OUTPUT_DIRECTORY || path.join(__dirname, 'current'),
);
fs.mkdirSync(outputDirectory, { recursive: true });

function loadProjectEsbuild() {
  try {
    return require(require.resolve('esbuild', { paths: [root] }));
  } catch (error) {
    throw new Error(`Cannot load project esbuild. Run npm ci in ${root} first.`, { cause: error });
  }
}

function loadPlaywright() {
  if (process.env.UM_PLAYWRIGHT_MODULE) {
    return require(path.resolve(process.env.UM_PLAYWRIGHT_MODULE));
  }
  try {
    return require(require.resolve('playwright', { paths: [root, __dirname] }));
  } catch (error) {
    throw new Error(
      'Cannot load Playwright. Set UM_PLAYWRIGHT_MODULE to an existing Playwright package directory.',
      { cause: error },
    );
  }
}

let esbuild;
let playwright;
module.exports = {
  root,
  outputDirectory,
  browserLaunchOptions: {
    headless: true,
    channel: 'msedge',
    args: ['--disable-gpu', '--disable-gpu-compositing'],
  },
  get esbuild() { return esbuild ||= loadProjectEsbuild(); },
  get playwright() { return playwright ||= loadPlaywright(); },
};
