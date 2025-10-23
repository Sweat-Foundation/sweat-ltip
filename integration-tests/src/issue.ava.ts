import { Account } from "./types.ts";
import { createTest } from "./setup.ts";

const test = createTest();

test('Check issue with `issue` call', async t => {
  const { contract, sweat, alice, bob, issuer } = t.context.accounts;

  {
    console.log('  ➤ View contract.get_spare_balance');
    const spareBalance = await contract.view('get_spare_balance');
    console.log('    ↩ Result:', spareBalance);

    t.is(spareBalance, '0');
  }

  {
    const amount = 1_000_000_000_000_000_000_000n;
    const issue_timestamp = 1761218300;

    console.log(`  ➤ Call contract.issue(alice, ${issue_timestamp}, ${amount.toString()}) by unauthorized account`);
    const result = await alice.callRaw(contract, 'issue', { issue_timestamp, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.failures);

    t.assert(result.receiptFailureMessagesContain('Unauthorized role'));

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account, null);
  }

  {
    const amount = 1_000_000_000_000_000_000_000n;
    const issue_timestamp = 1761218300;

    console.log(`  ➤ Call contract.issue(alice, ${issue_timestamp}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_timestamp, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.failures);

    t.assert(result.receiptFailureMessagesContain('Insufficient spare balance'));

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account, null);
  }

  {
    const topUpAmount = 10_000_000_000_000_000_000_000_000n;

    console.log(`  ➤ Call sweat.ft_transfer_call(top_up, ${topUpAmount.toString()}) by authorized account`);
    const result = await issuer.call(
      sweat, 'ft_transfer_call',
      { receiver_id: contract.accountId, amount: topUpAmount.toString(), msg: JSON.stringify({ type: 'top_up' }) },
      { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
    );
    console.log('    ↩ Result:', result);

    t.is(result, topUpAmount.toString());

    console.log('  ➤ View contract.get_spare_balance');
    const spareBalance = await contract.view('get_spare_balance');
    console.log('    ↩ Result:', spareBalance);

    t.is(spareBalance, topUpAmount.toString());
  }

  {
    const amount = 5_000_000_000_000_000_000_000_000n;
    const issue_timestamp = 1761218300;

    console.log(`  ➤ Call contract.issue(alice, ${issue_timestamp}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_timestamp, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.parseResult());

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account?.grants[issue_timestamp]?.total_amount, amount.toString());
  }

  {
    const amount = 500_000_000_000_000_000_000_000_000_000n;
    const issue_timestamp = 1761218300;

    console.log(`  ➤ Call contract.issue(bob, ${issue_timestamp}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_timestamp, grants: [[bob.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.failures);

    t.assert(result.receiptFailureMessagesContain('Insufficient spare balance'));

    console.log('  ➤ View contract.get_account(bob)');
    const account: Account | null = await contract.view('get_account', { account_id: bob.accountId });
    console.log('    ↩ Result:', account);

    t.is(account, null);
  }
});
