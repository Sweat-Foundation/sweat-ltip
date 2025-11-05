import { createTest } from "../setup.ts";
import { Account, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T11 ? Terminate in the future (timestamp between now and vesting end)', async t => {
  const { contract, ft, issuer, executor, alice } = t.context.accounts;

  // Given: G1.total_amount = 1_000, claimed_amount = 200, order_amount = 0
  // T_NOW < T_TERM < T_VEST_END, vested_at_T_TERM = 600
  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: sweat(10_000).toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  const T_ISSUE = now() - 900; // Issue 900 seconds ago
  await issuer.call(contract, 'issue', {
    issue_at: T_ISSUE,
    grants: [[alice.accountId, STANDARD_GRANT_TOTAL.toString()]]
  });

  // Setup claimed_amount = 200
  await t.context.worker.provider.fastForward(200);
  await alice.call(contract, 'claim', {});
  const account1: Account = await contract.view('get_account', { account_id: alice.accountId });
  const firstOrder = BigInt(account1.grants.at(0)!.order_amount);

  const authorizePercent1 = Math.floor(Number((sweat(200) * 10000n) / firstOrder));
  await executor.call(contract, 'authorize', {
    account_ids: [alice.accountId],
    percentage: authorizePercent1
  });

  const T_NOW = now();
  const T_TERM = T_NOW + 300; // Termination 300 seconds in the future
  const spareBefore = BigInt(await contract.view('get_spare_balance'));

  // When: Executor calls terminate(grant_id = G1, termination_timestamp = T_TERM)
  await executor.call(contract, 'terminate', {
    account_id: alice.accountId,
    timestamp: T_TERM
  });

  // Then: G1.total_amount = 600, terminated_at = T_TERM, spare increases by 400
  const account: Account = await contract.view('get_account', { account_id: alice.accountId });
  const G1 = account.grants.at(0)!;
  const vestedAtTerm = BigInt(G1.vested_amount); // Should be around 600 at T_TERM
  t.is(G1.total_amount, vestedAtTerm.toString());
  t.is(G1.terminated_at, T_TERM);
  t.is(G1.claimed_amount, account1.grants.at(0)!.claimed_amount);
  t.is(G1.order_amount, '0');

  const spareAfter = BigInt(await contract.view('get_spare_balance'));
  const delta = STANDARD_GRANT_TOTAL - BigInt(G1.total_amount);
  t.is(spareAfter, spareBefore + delta);
});
