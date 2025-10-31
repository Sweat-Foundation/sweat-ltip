import { createTest } from "../setup.ts";
import { Account, ft_balance, hasError, sweat } from "../types.ts";

const test = createTest(60, 180);
const amount = sweat(5_000);

test.skip('Check authorize', async t => {
  const { issuer, executor, contract, alice, ft } = t.context.accounts;

  const aliceStartingBalance = await ft_balance(ft, alice);

  {
    const issue_at = Math.floor(Date.now() / 1000);
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

  console.log(`  ➤ Wait 1_000 blocks`);
  await t.context.worker.provider.fastForward(1_000);

  {
    await alice.call(contract, 'claim', {});

    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    t.is(account.grants.at(0)?.order_amount, amount.toString());
  }

  t.is(await contract.view('get_spare_balance'), '0');

  {
    const result = await issuer.callRaw(contract, 'authorize', { account_ids: [alice.accountId], percentage: 10_000 });
    t.assert(hasError(result, 'Unauthorized role'));
    t.is(await contract.view('get_spare_balance'), '0');
  }

  {
    await executor.call(contract, 'authorize', { account_ids: [alice.accountId], percentage: 10_000 });

    t.is(await contract.view('get_spare_balance'), '0');

    const aliceCurrentBalance = await ft_balance(ft, alice);
    t.is(aliceCurrentBalance - aliceStartingBalance, amount);
  }
});
