import { createTest } from "../setup";
import { sweat } from "../types";

const test = createTest();

test('🧪 Continuous interaction', async t => {
  const { alice, issuer, executor, contract, ft } = t.context.accounts;

  await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: sweat(1_000).toString(), msg: JSON.stringify({ type: "top_up" }) },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );

  await issuer.call(contract, 'issue', { issue_at: 1704070800, grants: [[alice.accountId, sweat(1_000).toString()]] });

  await alice.call(contract, 'claim', {});

  await executor.call(contract, 'buy', { account_ids: [alice.accountId], percentage: 5_000 });

  await alice.call(contract, 'claim', {});

  await executor.call(contract, 'authorize', { account_ids: [alice.accountId], percentage: 5_000 });

  await alice.call(contract, 'claim', {});

  await contract.view('get_account', { account_id: alice.accountId });

  await executor.call(contract, 'terminate', { account_id: alice.accountId, timestamp: 1760918400 });

  await contract.view('get_account', { account_id: alice.accountId });
});
