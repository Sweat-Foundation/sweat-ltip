import { createTest } from "../setup.ts";
import { Account, ft_balance, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T10 ? Buy 100%', async t => {
  const { contract, ft, issuer, executor, alice } = t.context.accounts;

  // Precondition: G1.total_amount = 1_000, claimed_amount = 200, order_amount = 400
  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: sweat(10_000).toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  const T_ISSUE = now() - 500;
  await issuer.call(contract, 'issue', {
    issue_at: T_ISSUE,
    grants: [[alice.accountId, STANDARD_GRANT_TOTAL.toString()]]
  });

  // Setup: claimed_amount = 200, order_amount = 400
  await t.context.worker.provider.fastForward(200);
  await alice.call(contract, 'claim', {});
  const account1: Account = await contract.view('get_account', { account_id: alice.accountId });
  const firstOrder = BigInt(account1.grants.at(0)!.order_amount);

  const authorizePercent1 = Math.floor(Number((sweat(200) * 10000n) / firstOrder));
  await executor.call(contract, 'authorize', {
    account_ids: [alice.accountId],
    percentage: authorizePercent1
  });

  await t.context.worker.provider.fastForward(200);
  await alice.call(contract, 'claim', {});
  const account2: Account = await contract.view('get_account', { account_id: alice.accountId });
  const orderAmount = BigInt(account2.grants.at(0)!.order_amount);

  const spareBefore = BigInt(await contract.view('get_spare_balance'));
  const aliceBalanceBefore = await ft_balance(ft, alice);

  // When: Executor calls buy(grant_id = G1, percent = 100)
  await executor.call(contract, 'buy', {
    account_ids: [alice.accountId],
    percentage: 10_000 // 100%
  });

  // Then: Bought amount = 400, order_amount = 0, claimed_amount = 600, spare increases by 400
  const account: Account = await contract.view('get_account', { account_id: alice.accountId });
  const G1 = account.grants.at(0)!;
  t.is(G1.order_amount, '0');

  const expectedClaimed = BigInt(account2.grants.at(0)!.claimed_amount) + orderAmount;
  t.is(BigInt(G1.claimed_amount), expectedClaimed);

  const spareAfter = BigInt(await contract.view('get_spare_balance'));
  t.is(spareAfter, spareBefore + orderAmount);

  const aliceBalanceAfter = await ft_balance(ft, alice);
  t.is(aliceBalanceAfter, aliceBalanceBefore); // Employee balance unchanged
});
