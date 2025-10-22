import { createTest } from './setup.ts';

const test = createTest();

test('Check top up', async t => {
  console.log('🧪 Check top up');

  const { contract, sweat, issuer, alice } = t.context.accounts;
  const topUpMsg = JSON.stringify({ type: 'top_up' });

  t.is(await contract.view('get_spare_balance'), '0');

  console.log('  ➤ Call sweat.ft_transfer_call(top_up) by unauthorized account');
  const aliceTopUpAmount = 1000000;
  const aliceTopUpResult = await alice.callRaw(
    sweat, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: aliceTopUpAmount.toString(), msg: topUpMsg },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );
  console.log('    ↩ Result:', aliceTopUpResult.parseResult());

  t.is(Number(aliceTopUpResult.parseResult()), 0);
  t.is(await contract.view('get_spare_balance'), '0');

  console.log('  ➤ Call sweat.ft_transfer_call(top_up) by unauthorized account');
  const issuerTopUpAmount = 5000000;
  const issuerTopUpResult: string = await issuer.call(
    sweat, 'ft_transfer_call',
    { receiver_id: contract.accountId, amount: issuerTopUpAmount.toString(), msg: topUpMsg },
    { attachedDeposit: 1n, gas: BigInt(300 * 10 ** 12) }
  );
  console.log('    ↩ Result:', issuerTopUpResult);

  t.is(Number(issuerTopUpResult), issuerTopUpAmount);
  t.is(await contract.view('get_spare_balance'), issuerTopUpAmount.toString());
});
