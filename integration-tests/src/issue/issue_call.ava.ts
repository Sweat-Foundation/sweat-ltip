import { Account, sweat } from "../types.ts";
import { createTest } from "../setup.ts";

const test = createTest();

test('🧪 Check issue with `issue` call', async t => {
  const { contract, ft, alice, bob, issuer } = t.context.accounts;

  console.log('\n👞 Step one');
  {
    const spareBalance = await contract.view('get_spare_balance');

    t.is(spareBalance, '0');
  }

  console.log('\n👞 Step two');
  {
    const amount = sweat(1_000);
    const issue_at = 1761218300;

    const result = await alice.callRaw(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });
    t.assert(result.receiptFailureMessagesContain('Unauthorized role'));

    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    t.is(account, null);
  }

  console.log('\n👞 Step three');
  {
    const amount = sweat(1_000);
    const issue_at = 1761218300;

    const result = await issuer.callRaw(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });
    t.assert(result.receiptFailureMessagesContain('Insufficient spare balance'));

    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    t.is(account, null);
  }

  console.log('\n👞 Step four');
  {
    const topUpAmount = sweat(10_000_000);

    const result = await issuer.call(
      ft, 'ft_transfer_call',
      { receiver_id: contract.accountId, amount: topUpAmount.toString(), msg: JSON.stringify({ type: 'top_up' }) },
      { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
    );
    t.is(result, topUpAmount.toString());

    const spareBalance = await contract.view('get_spare_balance');
    t.is(spareBalance, topUpAmount.toString());
  }

  console.log('\n👞 Step five');
  {
    const amount = sweat(5_000_000);
    const issue_at = 1761218300;

    await issuer.call(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });

    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    t.is(account?.grants.at(0)?.total_amount, amount.toString());
  }

  console.log('\n👞 Step six');
  {
    const amount = sweat(500_000_000_000);
    const issue_at = 1761218300;

    const result = await issuer.callRaw(contract, 'issue', { issue_at, grants: [[bob.accountId, amount.toString()]] });
    t.assert(result.receiptFailureMessagesContain('Insufficient spare balance'));

    const account: Account | null = await contract.view('get_account', { account_id: bob.accountId });
    t.is(account, null);
  }
});
