
      <script setup>
      import { useCssVars, ref } from 'vue'
      const msg = ref()
      </script>

      <style>
      .foo {
        color: v-bind(msg)
      }
      </style>
      