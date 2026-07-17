use accesskit::{Action, Node, NodeId, Rect, Role, Tree, TreeId, TreeUpdate};
use aria_core::protocol::{UiRole, UiTree};

#[derive(Debug, Default)]
pub struct AccessTreeBuilder;

impl AccessTreeBuilder {
    #[must_use]
    pub fn build(&self, ui: &UiTree) -> TreeUpdate {
        let mut focus = NodeId(ui.root);
        let nodes = ui
            .nodes
            .values()
            .map(|ui_node| {
                let mut node = Node::new(match ui_node.role {
                    UiRole::Window => Role::Window,
                    UiRole::Group => Role::Group,
                    UiRole::Dialog => Role::Dialog,
                    UiRole::Label => Role::Label,
                    UiRole::Button => Role::Button,
                });
                node.set_label(ui_node.label.clone());
                node.set_bounds(Rect {
                    x0: f64::from(ui_node.bounds.x),
                    y0: f64::from(ui_node.bounds.y),
                    x1: f64::from(ui_node.bounds.x + ui_node.bounds.width),
                    y1: f64::from(ui_node.bounds.y + ui_node.bounds.height),
                });
                node.set_children(
                    ui_node
                        .children
                        .iter()
                        .copied()
                        .map(NodeId)
                        .collect::<Vec<_>>(),
                );
                if ui_node.focusable {
                    node.add_action(Action::Focus);
                }
                if ui_node.activation.is_some() {
                    node.add_action(Action::Click);
                }
                if ui_node.focused {
                    focus = NodeId(ui_node.id);
                }
                (NodeId(ui_node.id), node)
            })
            .collect();
        let mut tree = Tree::new(NodeId(ui.root));
        tree.toolkit_name = Some("AriaEngine".to_owned());
        tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").to_owned());
        TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: TreeId::ROOT,
            focus,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aria_core::InputAction;
    use aria_core::protocol::{Rect as CoreRect, UiActivation, UiNode};

    use super::*;

    #[test]
    fn focused_button_becomes_accesskit_focus_and_click_target() {
        let bounds = CoreRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 48.0,
        };
        let ui = UiTree {
            root: 1,
            nodes: BTreeMap::from([
                (
                    1,
                    UiNode {
                        id: 1,
                        role: UiRole::Window,
                        label: "game".to_owned(),
                        bounds,
                        focusable: false,
                        focused: false,
                        activation: None,
                        children: vec![2],
                    },
                ),
                (
                    2,
                    UiNode {
                        id: 2,
                        role: UiRole::Button,
                        label: "始める".to_owned(),
                        bounds,
                        focusable: true,
                        focused: true,
                        activation: Some(UiActivation::Input(InputAction::Confirm)),
                        children: Vec::new(),
                    },
                ),
            ]),
            scale_factor: 1.0,
            safe_area: bounds,
        };
        let update = AccessTreeBuilder.build(&ui);
        assert_eq!(update.focus, NodeId(2));
        let button = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .unwrap();
        assert!(button.1.supports_action(Action::Click));
    }
}
