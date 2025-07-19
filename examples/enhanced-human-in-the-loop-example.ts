import { cronflow } from '../sdk/src/index';
import { resume } from '../sdk/src/cronflow';

console.log('🚀 Enhanced Human-in-the-Loop Example\n');

// Define a comprehensive workflow that demonstrates enhanced human-in-the-loop features
const enhancedHumanWorkflow = cronflow.define({
  id: 'enhanced-human-approval-demo',
  name: 'Enhanced Human-in-the-Loop Demo',
  description:
    'Demonstrates enhanced human-in-the-loop capabilities with URLs, metadata, and resume functionality',
});

// Build the workflow with enhanced human-in-the-loop features
enhancedHumanWorkflow
  // Step 1: Initial transaction processing
  .step('process-transaction', async ctx => {
    console.log('💳 Processing transaction...');
    const transaction = {
      id: 'TXN-' + Date.now(),
      amount: 15000,
      customerId: 'CUST-123',
      customerTier: 'VIP',
      department: 'sales',
      description: 'Large enterprise deal',
    };
    return transaction;
  })

  // Step 2: Risk assessment
  .step('assess-risk', ctx => {
    console.log('🔍 Assessing transaction risk...');
    const transaction = ctx.last;

    const riskFactors = {
      amountRisk: transaction.amount > 10000 ? 'high' : 'low',
      customerRisk: transaction.customerTier === 'VIP' ? 'low' : 'medium',
      departmentRisk: transaction.department === 'sales' ? 'medium' : 'low',
    };

    const overallRisk =
      Object.values(riskFactors).filter(r => r === 'high').length > 0
        ? 'high'
        : 'medium';

    return {
      ...transaction,
      riskAssessment: riskFactors,
      overallRisk,
      requiresApproval: overallRisk === 'high' || transaction.amount > 10000,
    };
  })

  // Step 3: Conditional approval based on risk
  .if('requires-manager-approval', ctx => ctx.last.requiresApproval)
  .humanInTheLoop({
    timeout: '24h',
    description: 'Manager approval required for high-risk transaction',
    approvalUrl: 'https://approvals.company.com/manager-approve',
    metadata: {
      approvalLevel: 'manager',
      autoEscalate: true,
      escalationTime: '12h',
    },
    onPause: token => {
      console.log(`🛑 Manager approval required for high-risk transaction`);
      console.log(
        `🔗 Approval URL: https://approvals.company.com/manager-approve?token=${token}`
      );
      console.log(`⏰ Timeout: 24 hours (auto-escalation after 12h)`);
    },
  })
  .step('process-manager-approval', ctx => {
    console.log('✅ Manager approval processed');
    return {
      managerApproved: ctx.last.approved,
      managerReason: ctx.last.reason,
      managerApprovedBy: ctx.last.approvedBy,
      approvalLevel: ctx.last.metadata?.approvalLevel,
      riskLevel: ctx.last.metadata?.riskLevel,
    };
  })
  .else()
  .step('auto-approve', ctx => {
    console.log('✅ Transaction auto-approved (low risk)');
    return {
      autoApproved: true,
      reason: 'Auto-approved (below approval threshold)',
      approvedBy: 'system',
      approvalLevel: 'auto',
    };
  })
  .endIf()

  // Step 4: Additional approval for very high amounts
  .if('requires-director-approval', ctx => {
    const approvalStep =
      ctx.steps['process-manager-approval'] || ctx.steps['auto-approve'];
    return (
      approvalStep?.output?.managerApproved &&
      ctx.steps['process-transaction'].output.amount > 50000
    );
  })
  .humanInTheLoop({
    timeout: '48h',
    description: 'Director approval required for very high-value transaction',
    approvalUrl: 'https://approvals.company.com/director-approve',
    metadata: {
      approvalLevel: 'director',
      riskLevel: 'very_high',
      requiresBoardNotification: true,
    },
    onPause: token => {
      console.log(
        `🛑 Director approval required for very high-value transaction`
      );
      console.log(
        `🔗 Approval URL: https://approvals.company.com/director-approve?token=${token}`
      );
      console.log(`📢 Board notification will be sent upon approval`);
    },
  })
  .step('process-director-approval', ctx => {
    console.log('✅ Director approval processed');
    return {
      directorApproved: ctx.last.approved,
      directorReason: ctx.last.reason,
      directorApprovedBy: ctx.last.approvedBy,
      boardNotified: ctx.last.metadata?.requiresBoardNotification,
    };
  })
  .endIf()

  // Step 5: Final transaction processing
  .step('finalize-transaction', ctx => {
    console.log('🎯 Finalizing transaction...');

    const managerApproval = ctx.steps['process-manager-approval']?.output;
    const autoApproval = ctx.steps['auto-approve']?.output;
    const directorApproval = ctx.steps['process-director-approval']?.output;

    const approvalChain = [
      managerApproval && { level: 'manager', ...managerApproval },
      directorApproval && { level: 'director', ...directorApproval },
    ].filter(Boolean);

    const finalApproval = directorApproval || managerApproval || autoApproval;

    return {
      transactionId: ctx.steps['process-transaction'].output.id,
      amount: ctx.steps['process-transaction'].output.amount,
      customerId: ctx.steps['process-transaction'].output.customerId,
      approved:
        finalApproval?.managerApproved ||
        finalApproval?.directorApproved ||
        finalApproval?.autoApproved,
      approvalChain,
      riskLevel: ctx.steps['assess-risk'].output.overallRisk,
      finalizedAt: Date.now(),
      status: 'completed',
    };
  })

  // Step 6: Notification and logging
  .step('send-notifications', ctx => {
    console.log('📢 Sending notifications...');
    const transaction = ctx.last;

    if (transaction.approved) {
      console.log(
        `✅ Transaction ${transaction.transactionId} approved successfully`
      );
      console.log(`💰 Amount: $${transaction.amount.toLocaleString()}`);
      console.log(`👤 Customer: ${transaction.customerId}`);
      console.log(
        `🔗 Approval chain: ${transaction.approvalChain.length} levels`
      );
    } else {
      console.log(
        `❌ Transaction ${transaction.transactionId} was not approved`
      );
    }

    return {
      notificationsSent: true,
      transactionStatus: transaction.approved ? 'approved' : 'rejected',
      timestamp: Date.now(),
    };
  });

// Simulate external approval process
async function simulateExternalApproval() {
  console.log('\n🔄 Simulating external approval process...');

  // Simulate a manager approval
  const managerToken = 'manager_approval_token_' + Date.now();
  const managerPayload = {
    approved: true,
    reason: 'Transaction looks good, customer is VIP',
    approvedBy: 'sarah.manager@company.com',
    comments: 'Customer has good payment history',
  };

  console.log(`📧 Manager receives approval email with token: ${managerToken}`);
  console.log(
    `🌐 Manager clicks approval URL: https://approvals.company.com/manager-approve?token=${managerToken}`
  );

  // Simulate resume call
  await resume(managerToken, managerPayload);

  // Simulate a director approval for high-value transaction
  const directorToken = 'director_approval_token_' + Date.now();
  const directorPayload = {
    approved: true,
    reason: 'Large deal approved after review',
    approvedBy: 'john.director@company.com',
    comments: 'Strategic customer, good terms',
  };

  console.log(
    `📧 Director receives approval email with token: ${directorToken}`
  );
  console.log(
    `🌐 Director clicks approval URL: https://approvals.company.com/director-approve?token=${directorToken}`
  );

  // Simulate resume call
  await resume(directorToken, directorPayload);
}

// Start the engine and trigger the workflow
async function runEnhancedHumanInTheLoopDemo() {
  try {
    console.log('🚀 Starting Enhanced Human-in-the-Loop Demo...\n');

    // Start the engine
    await cronflow.start();

    // Trigger the workflow
    const result = await cronflow.trigger('enhanced-human-approval-demo', {
      demo: true,
      timestamp: Date.now(),
    });

    console.log('\n✅ Workflow triggered successfully!');
    console.log(`📋 Run ID: ${result}`);

    // Simulate external approval process
    await simulateExternalApproval();

    // Wait a moment for processing
    await new Promise(resolve => setTimeout(resolve, 2000));

    console.log('\n📊 Demo Summary:');
    console.log('✅ Enhanced human-in-the-loop features demonstrated:');
    console.log('  • Token generation and management');
    console.log('  • Approval URL support for web-based approvals');
    console.log('  • Rich metadata for approval context');
    console.log('  • Resume functionality for external approval handling');
    console.log('  • Conditional approval workflows');
    console.log('  • Multiple approval levels (manager, director)');
    console.log('  • Timeout handling with expiration tracking');
    console.log('  • Approval chain tracking');
    console.log('  • Risk-based approval routing');
    console.log('  • Auto-escalation capabilities');
  } catch (error) {
    console.error('❌ Demo failed:', error);
  }
}

// Run the demo
runEnhancedHumanInTheLoopDemo();
