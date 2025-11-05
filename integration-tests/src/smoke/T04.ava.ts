import { createTest } from "../setup.ts";
import { Account, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T04 ? Claim creates order for vested minus claimed', async t => {
  const { contract, ft, issuer, executor, alice } = t.context.accounts;

  // Given: alice has grant G1 with claimed_amount = 200, order_amount = 0
  // Time is during vesting: T_CLIFF_END < T_NOW < T_VEST_END, vested(T_NOW) = 600
  const topUpAmount = sweat(5_000);
  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: topUpAmount.toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  const T_ISSUE = now() - 500; // Issue 500 seconds ago (after cliff, during vesting)
  await issuer.call(contract, 'issue', {
    issue_at: T_ISSUE,
    grants: [[alice.accountId, STANDARD_GRANT_TOTAL.toString()]]
  });

  // Create claimed_amount = 200 by claiming and authorizing partially
  await t.context.worker.provider.fastForward(200); // Advance time
  await alice.call(contract, 'claim', {});
  const account1: Account = await contract.view('get_account', { account_id: alice.accountId });
  const firstOrderAmount = BigInt(account1.grants.at(0)!.order_amount);

  // Authorize 200 tokens worth
  const authorizePercent = Math.floor(Number((sweat(200) * 10000n) / firstOrderAmount));
  await executor.call(contract, 'authorize', {
    account_ids: [alice.accountId],
    percentage: authorizePercent
  });

  // Now claim again to create new order
  await t.context.worker.provider.fastForward(200); // Advance time more
  const T_NOW = now();

  // When: alice calls claim(grant_id = G1)
  await alice.call(contract, 'claim', {});

  // Then: G1.order_amount = vested - claimed, claimed_amount remains 200
  const account: Account = await contract.view('get_account', { account_id: alice.accountId });
  const G1 = account.grants.at(0)!;
  const claimedAmount = BigInt(G1.claimed_amount);
  const orderAmount = BigInt(G1.order_amount);
  const vestedAmount = BigInt(G1.vested_amount);

  // Verify order_amount = vested - claimed
  t.is(orderAmount.toString(), (vestedAmount - claimedAmount).toString());
  t.is(G1.claimed_amount, account1.grants.at(0)!.claimed_amount); // Remains same
  t.is(await contract.view('get_spare_balance'), topUpAmount.toString());
});
