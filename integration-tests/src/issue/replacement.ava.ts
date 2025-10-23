import { Account, sweat } from "../types.ts";
import { createTest } from "../setup.ts";

const test = createTest();

test('Check grants replacement', async t => {
  const { contract, ft, alice, bob, issuer } = t.context.accounts;

  console.log('\n👞 Step one');
  {
    const topUpAmount = sweat(10_000_000);

    console.log(`  ➤ Call sweat.ft_transfer_call(top_up, ${topUpAmount.toString()}) by authorized account`);
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

  const targetAmount = sweat(100);
  const issue_at = 1761218300;

  console.log('\n👞 Step two');
  {
    const amount = targetAmount;

    console.log(`  ➤ Call contract.issue(alice, ${issue_at}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.parseResult());

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account?.grants[issue_at]?.total_amount, targetAmount.toString());
  }

  console.log('\n👞 Step three');
  {
    const amount = targetAmount / 2n;

    console.log(`  ➤ Call contract.issue(alice, ${issue_at}, ${amount.toString()}) by authorized account`);
    const result = await issuer.callRaw(contract, 'issue', { issue_at: issue_at, grants: [[alice.accountId, amount.toString()]] });
    console.log('    ↩ Result:', result.failures);

    t.assert(result.receiptFailureMessagesContain('A grant has alredy been issued on this date'));

    console.log('  ➤ View contract.get_account(alice)');
    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account?.grants[issue_at]?.total_amount, targetAmount.toString());
  }
});
