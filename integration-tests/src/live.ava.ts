import { createProdMirrotTest } from "./setup.ts";

const test = createProdMirrotTest();

test('Check config', async t => {
  const { contract } = t.context.accounts;

  const config = await contract.view('get_config');
  console.log(config);

  const account = await contract.view('get_account', { account_id: 'honkbonk.near' });
  console.log(account);
});
