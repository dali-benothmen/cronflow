#!/usr/bin/env bun

/**
 * Node-Cronflow Examples Runner
 *
 * This file runs all the workflow examples to demonstrate
 * the various features of the Node-Cronflow SDK.
 */

import './01-simple-webhook';
import './02-scheduled-workflow';
import './03-robust-workflow';
import './04-complex-workflow';
import './05-control-flow-workflow';

console.log('\n🎉 All examples executed successfully!');
console.log('\n📊 Summary:');
console.log('  ✅ Simple Webhook Workflow');
console.log('  ✅ Scheduled Workflow');
console.log('  ✅ Robust Workflow (with retry/timeout)');
console.log('  ✅ Complex Multi-Trigger Workflow');
console.log('  ✅ Control Flow Workflow (if/else/parallel/race/while)');
console.log('\n🚀 Ready to build powerful workflows with Node-Cronflow!');
