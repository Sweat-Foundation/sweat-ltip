import { createTest } from './setup.ts';

const test = createTest();

test('🧪 Check top up', async t => {
  const { contract, ft, issuer, alice } = t.context.accounts;
  const topUpMsg = JSON.stringify({ type: 'top_up' });

  t.is(await contract.view('get_spare_balance'), '0');

  const aliceTopUpAmount = 1000000;
  const aliceTopUpResult = await alice.callRaw(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: aliceTopUpAmount.toString(), msg: topUpMsg },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );
  t.is(Number(aliceTopUpResult.parseResult()), 0);
  t.is(await contract.view('get_spare_balance'), '0');

  const issuerTopUpAmount = 5000000;
  const issuerTopUpResult: string = await issuer.call(
    ft, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: issuerTopUpAmount.toString(), msg: topUpMsg },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );
  t.is(Number(issuerTopUpResult), issuerTopUpAmount);
  t.is(await contract.view('get_spare_balance'), issuerTopUpAmount.toString());
});
