{#await thePromise}
	<p>loading...</p>
{:then theValue}
	<p>thex value is {theValue}</p>
{:catch theError}
	<p>oh no! {theError.message}</p>
{/await}