import { Account, sweat } from "../types.ts";
import { createTest } from "../setup.ts";

const test = createTest();

test('🧪 Check grants replacement', async t => {
  const { contract, ft, alice, bob, issuer } = t.context.accounts;

  console.log('\n👞 Step one');
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

  const targetAmount = sweat(100);
  const issue_at = 1761218300;

  console.log('\n👞 Step two');
  {
    const amount = targetAmount;

    await issuer.call(contract, 'issue', { issue_at, grants: [[alice.accountId, amount.toString()]] });

    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    t.is(account?.grants.at(0)?.total_amount, targetAmount.toString());
  }

  console.log('\n👞 Step three');
  {
    const amount = targetAmount / 2n;

    const result = await issuer.callRaw(contract, 'issue', { issue_at: issue_at, grants: [[alice.accountId, amount.toString()]] });
    t.assert(result.receiptFailureMessagesContain('A grant has alredy been issued on this date'));

    const account: Account | null = await contract.view('get_account', { account_id: alice.accountId });
    t.is(account?.grants.at(0)?.total_amount, targetAmount.toString());
  }
});
