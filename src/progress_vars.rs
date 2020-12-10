use std::collections::HashMap;
use std::any::Any;
use std::collections::hash_map::Drain;

pub type ProgressVar = Box<dyn Any + Send>;
#[derive(Debug, Default)]
pub struct ProgressVars {
    vars: HashMap<String, ProgressVar>,
}

#[macro_export]
macro_rules! delegate_progressvars_on {
    ($name:tt) => {
        delegate ! {
            to self.$name {
                pub fn extract_var<S: AsRef<str>>(&mut self, key: S) -> Option<Box<dyn Any + Send>>;
                pub fn insert_var<S: AsRef<str>>(&mut self, key: S, boxed: Box<dyn Any + Send>);
                pub fn var_exists<S: AsRef<str>>(&self, key: S) -> bool;
                pub fn drain_vars(&mut self) -> Drain<'_, String, Box<(dyn Any + Send)>>;
            }
        }
    };
}

impl ProgressVars {
    /// this returns the Box<dyn Any> that
    /// is referenced by the key. if you want this
    /// variable to be reused later, you must
    /// reinsert it by calling insert_var
    pub fn extract_var<S: AsRef<str>>(
        &mut self,
        key: S
    ) -> Option<Box<dyn Any + Send>>{
        self.vars.remove(key.as_ref())
    }

    /// creates a string from your key to be inserted
    /// into the vars hashmap. no check is done to see
    /// if the variable exists prior to inserting, so
    /// that is up to you to do by checking var_exists if desired
    pub fn insert_var<S: AsRef<str>>(&mut self, key: S, boxed: Box<dyn Any + Send>) {
        self.vars.insert(key.as_ref().to_string(), boxed);
    }

    pub fn var_exists<S: AsRef<str>>(&self, key: S) -> bool {
        self.vars.contains_key(key.as_ref())
    }

    pub fn drain_vars(&mut self) -> Drain<'_, String, Box<(dyn Any + Send)>> {
        self.vars.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_use_progress_vars() {
        let mut progvars = ProgressVars::default();
        let key = "key";
        progvars.insert_var(
            String::from(key),
            Box::new(String::from("some value"))
        );
        let boxed = progvars.extract_var(key).unwrap();
        if let Ok(x) = boxed.downcast::<String>() {
            assert_eq!(x.as_str(), "some value");
        } else {
            assert!(false);
        }
    }
}
