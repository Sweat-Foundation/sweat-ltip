import { createTest } from "../setup.ts";
import { Account, hasError, sweat } from "../types.ts";

const test = createTest(60, 180);
const amount = sweat(5_000);

test('Check buy', async t => {
  const { issuer, executor, contract, alice, ft } = t.context.accounts;

  const issue_at = Math.floor(Date.now() / 1000);
  const msg = JSON.stringify({
    type: 'issue',
    data: {
      issue_at,
      grants: [[alice.accountId, amount.toString()]]
    }
  });

  {
    console.log(`  ➤ Call ft.ft_transfer_call(${contract.accountId}, ${amount.toString()}, ${msg}) by authorized account`);
    const result = await issuer.call(
      ft, 'ft_transfer_call',
      { receiver_id: contract.accountId, amount: amount.toString(), msg },
      { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
    );
    console.log('    ↩ Result:', result);
  }

  console.log(`  ➤ Wait 1_000 blocks`);
  await t.context.worker.provider.fastForward(1_000);

  {
    console.log(`  ➤ Call contract.claim by alice`);
    await alice.call(contract, 'claim', {});

    console.log(`  ➤ View contract.get_account(alice)`);
    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.is(account.grants[issue_at].order_amount, amount.toString());
  }

  t.is(await contract.view('get_spare_balance'), '0');

  {
    console.log(`  ➤ Call contract.buy by unauthorized account`);
    const result = await issuer.callRaw(contract, 'buy', { account_ids: [alice.accountId], percentage: 10_000 });
    console.log('    ↩ Result:', result.failures);

    t.assert(hasError(result, 'Unauthorized role'));
    t.is(await contract.view('get_spare_balance'), '0');
  }

  {
    console.log(`  ➤ Call contract.buy by unauthorized account`);
    const result = await executor.call(contract, 'buy', { account_ids: [alice.accountId], percentage: 10_000 });
    console.log('    ↩ Result:', result);

    t.is(await contract.view('get_spare_balance'), amount.toString());
  }
});
