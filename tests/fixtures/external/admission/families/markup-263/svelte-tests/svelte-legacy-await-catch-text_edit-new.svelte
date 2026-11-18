{#await thePromise}
	<p>loading...</p>
{:catch theError}
	<p>ohx no! {theError.message}</p>
{/await}