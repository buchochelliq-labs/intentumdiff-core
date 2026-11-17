
        <script setup>
        import ChildComp from './Child.vue'
        import SomeOtherComp from './Other.vue'
        import vMyDir from './my-dir'
        </script>
        <template>
          <div v-my-dir></div>
          <ChildComp/>
          <some-other-comp/>
        </template>
        