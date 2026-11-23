# Agent Context Workshop

Open [project.json](project.json) with **File → Open Project** in the Windows Editor. The startup Scene is **north**. In the tree, select **Signal Cap** and open the Agent drawer. The standalone object uses the key `status-sphere` and has X position −2.

Open **south** from **Game → Scene Information**. Its standalone **Signal Cap** has the same key and X position 2. A third **Signal Cap** is the mesh within the pinned Prop. Expand the linked instance, **Gate Frame**, and the **Signal Cap** placement wrapper, then select the child **Signal Cap**. The Agent should address the child’s linked member and its `mesh` component. Select another object during a request to check that the answer keeps the original subject.

The linked Prop resources and primitives come from [Primitive Workshop](../primitive-workshop/README.md). See [Testing and validation](../../../../../docs/testing.md) for release verification.
