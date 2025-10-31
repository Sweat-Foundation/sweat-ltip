import { createProdMirrotTest } from "./setup.ts";

const test = createProdMirrotTest();

test('🧪 Check config', async t => {
  const { contract } = t.context.accounts;

  const config = await contract.view('get_config');
  console.log(config);

  const account = await contract.view('get_account', { account_id: '595c44dabb11565fba71009828aceb3671946dc97509caa90181e96206e25263' });
  console.log(account);
});
