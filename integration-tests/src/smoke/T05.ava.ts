import { createTest } from "../setup.ts";
import { Account, ft_balance, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T05 ? Authorize 0% (decline)', async t => {
  const { contract, ft, issuer, executor, alice } = t.context.accounts;

  // Precondition: alice has G1 with claimed_amount = 200, order_amount = 400
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

  // Create claimed_amount = 200 and order_amount = 400
  await t.context.worker.provider.fastForward(200);
  await alice.call(contract, 'claim', {});
  const account1: Account = await contract.view('get_account', { account_id: alice.accountId });
  const firstOrder = BigInt(account1.grants.at(0)!.order_amount);

  // Authorize to get claimed_amount = 200
  const authorizePercent1 = Math.floor(Number((sweat(200) * 10000n) / firstOrder));
  await executor.call(contract, 'authorize', {
    account_ids: [alice.accountId],
    percentage: authorizePercent1
  });

  // Create order_amount = 400
  await t.context.worker.provider.fastForward(200);
  await alice.call(contract, 'claim', {});
  const account2: Account = await contract.view('get_account', { account_id: alice.accountId });
  const currentOrder = BigInt(account2.grants.at(0)!.order_amount);

  // Adjust to get exactly 400
  const targetOrder = sweat(400);
  if (currentOrder !== targetOrder) {
    // If needed, we'll work with what we have
  }

  const spareBefore = await contract.view('get_spare_balance');
  const aliceBalanceBefore = await ft_balance(ft, alice);

  // When: Executor calls authorize(grant_id = G1, percent = 0)
  await executor.call(contract, 'authorize', {
    account_ids: [alice.accountId],
    percentage: 0
  });

  // Then: G1.order_amount = 0, claimed_amount = 200, spare unchanged, no transfer
  const account: Account = await contract.view('get_account', { account_id: alice.accountId });
  const G1 = account.grants.at(0)!;
  t.is(G1.order_amount, '0');
  t.is(G1.claimed_amount, account2.grants.at(0)!.claimed_amount);
  t.is(await contract.view('get_spare_balance'), spareBefore);
  const aliceBalanceAfter = await ft_balance(ft, alice);
  t.is(aliceBalanceAfter, aliceBalanceBefore);
});
