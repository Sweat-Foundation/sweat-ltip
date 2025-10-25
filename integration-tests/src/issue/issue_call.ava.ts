import { Account, sweat } from "../types.ts";
import { createTest } from "../setup.ts";

const test = createTest();

test('Check issue with `issue` call', async t => {
  const { contract, ft, alice, bob, issuer } = t.context.accounts;

  console.log('\n👞 Step one');
  {
    console.log('  ➤ View contract.get_spare_balance');
    const spareBalance = await contract.view('get_spare_balance');
    console.log('    ↩ Result:', spareBalance);

    t.is(spareBalance, '0');
  }

  console.log('\n👞 Step two');
  {
    const amount = sweat(1_000);
    const issue_at = 1761218300;

    console.log(`  ➤ Call contract.issue(alice, ${issue_at}, ${amount.toString()}) by unauthorized account`);
    const result = await alice.callRaw(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.failures);

    t.assert(result.receiptFailureMessagesContain('Unauthorized role'));

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account, null);
  }

  console.log('\n👞 Step three');
  {
    const amount = sweat(1_000);
    const issue_at = 1761218300;

    console.log(`  ➤ Call contract.issue(alice, ${issue_at}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.failures);

    t.assert(result.receiptFailureMessagesContain('Insufficient spare balance'));

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account, null);
  }

  console.log('\n👞 Step four');
  {
    const topUpAmount = sweat(10_000_000);

    console.log(`  ➤ Call ft.ft_transfer_call(top_up, ${topUpAmount.toString()}) by authorized account`);
    const result = await issuer.call(
      ft, 'ft_transfer_call',
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

  console.log('\n👞 Step five');
  {
    const amount = sweat(5_000_000);
    const issue_at = 1761218300;

    console.log(`  ➤ Call contract.issue(alice, ${issue_at}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.parseResult());

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account?.grants[issue_at]?.total_amount, amount.toString());
  }

  console.log('\n👞 Step six');
  {
    const amount = sweat(500_000_000_000);
    const issue_at = 1761218300;

    console.log(`  ➤ Call contract.issue(bob, ${issue_at}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_at, grants: [[bob.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.failures);

    t.assert(result.receiptFailureMessagesContain('Insufficient spare balance'));

    console.log('  ➤ View contract.get_account(bob)');
    const account: Account | null = await contract.view('get_account', { account_id: bob.accountId });
    console.log('    ↩ Result:', account);

    t.is(account, null);
  }
});
