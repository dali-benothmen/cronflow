#!/usr/bin/env node

/**
 * Test script to verify the npm package works correctly
 * Run with: node scripts/test-package.js
 */

const path = require('path');
const fs = require('fs');

console.log('🧪 Testing Node-Cronflow package...\n');

// Test 1: Check if dist files exist
console.log('1. Checking build output...');
const distPath = path.join(__dirname, '../dist');
if (!fs.existsSync(distPath)) {
  console.error('❌ dist/ directory not found. Run "npm run build" first.');
  process.exit(1);
}

const requiredFiles = [
  'index.js',
  'index.d.ts',
  'sdk/index.js',
  'sdk/index.d.ts',
  'services/index.js',
  'services/index.d.ts',
];

for (const file of requiredFiles) {
  const filePath = path.join(distPath, file);
  if (!fs.existsSync(filePath)) {
    console.error(`❌ Missing required file: ${file}`);
    process.exit(1);
  }
}
console.log('✅ All required files exist');

// Test 2: Try to require the package
console.log('\n2. Testing package import...');
try {
  const cronflow = require('../dist/index.js');
  console.log('✅ Package imports successfully');
  console.log('📦 Exported keys:', Object.keys(cronflow));
} catch (error) {
  console.error('❌ Failed to import package:', error.message);
  process.exit(1);
}

// Test 3: Check package.json
console.log('\n3. Checking package.json...');
const packageJson = require('../package.json');
const requiredFields = ['name', 'version', 'main', 'types', 'files'];
for (const field of requiredFields) {
  if (!packageJson[field]) {
    console.error(`❌ Missing required field: ${field}`);
    process.exit(1);
  }
}
console.log('✅ Package.json has all required fields');

// Test 4: Check file size
console.log('\n4. Checking package size...');
const stats = fs.statSync(path.join(distPath, 'index.js'));
const sizeKB = Math.round(stats.size / 1024);
console.log(`📦 Main bundle size: ${sizeKB}KB`);

if (sizeKB > 1000) {
  console.warn('⚠️  Package size is large (>1MB). Consider optimization.');
} else {
  console.log('✅ Package size is reasonable');
}

console.log('\n🎉 All tests passed! Package is ready for publishing.');
console.log('\nTo publish:');
console.log('1. npm login');
console.log('2. npm publish');
