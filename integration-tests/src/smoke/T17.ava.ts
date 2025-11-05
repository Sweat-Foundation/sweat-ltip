import { createTest } from "../setup.ts";
import { Account, now, sweat } from "../types.ts";

const test = createTest(100, 900); // cliff_seconds = 100, vesting_seconds = 900
const STANDARD_GRANT_TOTAL = sweat(1_000);

test('T17 ? Terminate before cliff (after issue, before cliff end)', async t => {
  const { contract, ft, issuer, executor, alice } = t.context.accounts;

  // Given: T_ISSUE < T_TERM < T_CLIFF_END, G1.total_amount = 1_000
  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: sweat(10_000).toString(), msg: JSON.stringify({ type: 'top_up' }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  const T_ISSUE = now() - 50; // Issue 50 seconds ago (before cliff end at 100)
  await issuer.call(contract, 'issue', {
    issue_at: T_ISSUE,
    grants: [[alice.accountId, STANDARD_GRANT_TOTAL.toString()]]
  });

  const T_TERM = T_ISSUE + 50; // Termination 50 seconds after issue (before cliff end)
  const spareBefore = BigInt(await contract.view('get_spare_balance'));

  // When: Executor calls terminate(grant_id = G1, termination_timestamp = T_TERM)
  await executor.call(contract, 'terminate', {
    account_id: alice.accountId,
    timestamp: T_TERM
  });

  // Then: G1.total_amount = 0, all tokens return to spare
  const account: Account = await contract.view('get_account', { account_id: alice.accountId });
  const G1 = account.grants.at(0)!;
  t.is(G1.total_amount, '0');
  t.is(G1.claimed_amount, '0');
  t.is(G1.order_amount, '0');
  t.is(G1.terminated_at, T_TERM);

  const spareAfter = BigInt(await contract.view('get_spare_balance'));
  t.is(spareAfter, spareBefore + STANDARD_GRANT_TOTAL);
});
