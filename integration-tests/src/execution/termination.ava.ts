import { createTest } from "../setup.ts";
import { Account, ft_balance, hasError, sweat } from "../types.ts";

const test = createTest(0, 86_400);
const amount = sweat(86_400);

test('Check termination', async t => {
  const { issuer, executor, contract, alice, ft } = t.context.accounts;

  const issue_at = Math.floor(Date.now() / 1000);
  const terminate_at = issue_at + 1_000;

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

  console.log(`  ➤ Wait 500 blocks`);
  await t.context.worker.provider.fastForward(500);

  {
    console.log(`  ➤ Call contract.claim by alice`);
    await alice.call(contract, 'claim', {});

    console.log(`  ➤ View contract.get_account(alice)`);
    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    t.assert(BigInt(account.grants[issue_at].order_amount) > 0n);
  }

  t.is(await contract.view('get_spare_balance'), '0');

  {
    console.log(`  ➤ Call contract.terminate by unauthorized account`);
    const result = await issuer.callRaw(contract, 'terminate', { account_id: alice.accountId, timestamp: terminate_at });
    console.log('    ↩ Result:', result.failures);

    t.assert(hasError(result, 'Unauthorized role'));
    t.is(await contract.view('get_spare_balance'), '0');
  }

  {
    console.log(`  ➤ Call contract.terminate by authorized account`);
    const result = await executor.call(contract, 'terminate', { account_id: alice.accountId, timestamp: terminate_at });
    console.log('    ↩ Result:', result);

    console.log(`  ➤ View contract.get_account(alice)`);
    const account: Account = await contract.view('get_account', { account_id: alice.accountId });
    console.log('    ↩ Result:', account);

    const aliceTotalAmountAfterTermination = BigInt(account.grants[issue_at].total_amount);
    t.assert(aliceTotalAmountAfterTermination, sweat(1_000).toString());
    t.is(account.grants[issue_at].order_amount, '0');
    t.is(account.grants[issue_at].claimed_amount, '0');

    console.log(`  ➤ View contract.get_spare_balance`);
    const spareBalance = await contract.view('get_spare_balance');
    console.log('    ↩ Result:', spareBalance);

    t.is(spareBalance, (amount - aliceTotalAmountAfterTermination).toString());
  }
});
