// Compatibility entry point; the current selection lives in run-regression.cjs.
try { require('./run-regression.cjs').main(); }
catch (error) { console.error(error.stack); process.exitCode = 1; }
