
        <script setup>
        import Foo from './foo'
        </script>
        <template>
          <div>{{ new Foo() }}</div>
          <div>{{ new Foo.Bar() }}</div>
        </template>
        