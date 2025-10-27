import { createTest } from "../setup.ts";
import { Account, hasError, sweat } from "../types.ts";

const test = createTest(0, 86_400);
const amount = sweat(86_400);

test.skip('Check termination', async t => {
  const { issuer, executor, contract, alice, ft } = t.context.accounts;

  const issue_at = Math.floor(Date.now() / 1000);
  const terminate_at = issue_at + 1_000;

  {
    const msg = JSON.stringify({
      type: 'issue',
      data: {
        issue_at,
        grants: [[alice.accountId, amount.toString()]]
      }
    });

    await issuer.call(
      ft, 'ft_transfer_call',
      { receiver_id: contract.accountId, amount: amount.toString(), msg },
      { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
    );
  }

  console.log(`  ➤ Wait 500 blocks`);
  await t.context.worker.provider.fastForward(500);

  {
    await alice.call(contract, 'claim', {});

    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    t.assert(BigInt(account.grants.at(0)!.order_amount) > 0n);
  }

  t.is(await contract.view('get_spare_balance'), '0');

  {
    const result = await issuer.callRaw(contract, 'terminate', { account_id: alice.accountId, timestamp: terminate_at });
    t.assert(hasError(result, 'Unauthorized role'));
    t.is(await contract.view('get_spare_balance'), '0');
  }

  {
    await executor.call(contract, 'terminate', { account_id: alice.accountId, timestamp: terminate_at });

    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    const aliceTotalAmountAfterTermination = BigInt(account.grants[issue_at].total_amount);
    t.assert(aliceTotalAmountAfterTermination, sweat(1_000).toString());
    t.is(account.grants[issue_at].order_amount, '0');
    t.is(account.grants[issue_at].claimed_amount, '0');

    const spareBalance = await contract.view('get_spare_balance');
    t.is(spareBalance, (amount - aliceTotalAmountAfterTermination).toString());
  }
});
