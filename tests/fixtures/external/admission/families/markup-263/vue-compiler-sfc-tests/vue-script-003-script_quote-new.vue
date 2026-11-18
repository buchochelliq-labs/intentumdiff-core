
      <script setup>
      import { reactive } from "vue"
      const name = reactive({ first: 'john', last: 'doe' })
      </script>

      <template>
        <MyComponent v-model="name" />
      </template>
    