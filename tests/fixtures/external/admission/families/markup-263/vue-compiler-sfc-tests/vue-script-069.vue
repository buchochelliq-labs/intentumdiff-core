
      <script setup>
      import { ref as r } from 'vue'
      defineProps({
        foo: String
      })

      const a = r(1)
      let b = 2
      const c = 3
      const { d } = someFoo()
      let { e } = someBar()
      </script>
    